//! Species analysis (maze-stats): aggregate a `tags.csv` that carries the extended
//! optional columns — `project`, `collection`, `deployment`, `species`, `event_id`
//! — into per-species counts, filtered by project / collection / deployment.
//!
//! Self-contained (holds its own polars `DataFrame`) so it doesn't disturb the
//! time-analysis pipeline in `data.rs`. Two metrics, straight from `tags.csv`:
//!   - detections = row count per species ("有效探测次数")
//!   - captures   = distinct `event_id` per species ("独立捕获次数")
//! Columns are matched case-insensitively and are all optional; a plain
//! time-analysis `tags.csv` (no `species`) simply reports `has_species() == false`.

use anyhow::{Context, Result, anyhow, bail};
use polars::prelude::*;
use polars_io::prelude::{CsvReadOptions, CsvWriter, SerReader, SerWriter};
use std::collections::HashMap;
use std::io::Cursor;

/// Sent by the UI when there is no `project` column (or "all projects").
pub const PROJECT_ALL: &str = "__all__";

pub struct SpeciesData {
    frame: DataFrame,
    cols: HashMap<String, String>, // lowercased name -> actual column name
}

impl SpeciesData {
    pub fn from_csv_text(csv: &str) -> Result<Self> {
        let frame = CsvReadOptions::default()
            .with_has_header(true)
            .with_infer_schema_length(Some(256))
            .into_reader_with_file_handle(Cursor::new(csv.as_bytes()))
            .finish()
            .context("failed to read CSV for species analysis")?;
        let cols = frame
            .get_column_names()
            .into_iter()
            .map(|name| (name.to_lowercase(), name.to_string()))
            .collect();
        Ok(Self { frame, cols })
    }

    /// Actual column name for a canonical key, if present (case-insensitive).
    fn actual(&self, canonical: &str) -> Option<&str> {
        self.cols.get(canonical).map(String::as_str)
    }

    pub fn has_species(&self) -> bool { self.actual("species").is_some() }
    pub fn has_event_id(&self) -> bool { self.actual("event_id").is_some() }
    pub fn has_collection(&self) -> bool { self.actual("collection").is_some() }

    /// Distinct, non-empty, sorted values of a column (cast to string first, so a
    /// numeric deployment/collection id still works).
    fn unique_strings(&self, actual_name: &str) -> Result<Vec<String>> {
        let series = self
            .frame
            .column(actual_name)?
            .as_materialized_series()
            .cast(&DataType::String)?;
        let mut out: Vec<String> = series
            .str()?
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Projects to choose from. Empty column → a single synthetic "all".
    pub fn projects(&self) -> Result<Vec<String>> {
        match self.actual("project") {
            Some(name) => self.unique_strings(name),
            None => Ok(vec![PROJECT_ALL.to_string()]),
        }
    }

    /// Collections + deployments available within a project (for the filters).
    pub fn project_summary(&self, project: &str) -> Result<(Vec<String>, Vec<String>)> {
        let mut lf = self.frame.clone().lazy();
        if project != PROJECT_ALL {
            if let Some(name) = self.actual("project") {
                lf = lf.filter(col(name).cast(DataType::String).eq(lit(project.to_string())));
            }
        }
        let scoped = SpeciesData {
            frame: lf.collect().context("failed to scope project")?,
            cols: self.cols.clone(),
        };
        let collections = scoped.actual("collection").map(|c| scoped.unique_strings(c)).transpose()?.unwrap_or_default();
        let deployments = scoped.actual("deployment").map(|c| scoped.unique_strings(c)).transpose()?.unwrap_or_default();
        Ok((collections, deployments))
    }

    pub fn has_taxonomy(&self) -> bool { self.actual("classcn").is_some() }

    /// The distinct values for one taxon level (`class`/`family`/`genus`/
    /// `species`) in scope, after `hide_non_animals` and the filters for the
    /// levels *above* this one (so the dropdowns cascade: family narrows to the
    /// chosen class, genus to class+family, etc.). Empty values are dropped.
    pub fn taxonomy_values(
        &self,
        level: &str,
        project: &str,
        collections: &[String],
        deployments: &[String],
        taxon_filters: &[(String, Vec<String>)],
        hide_non_animals: bool,
    ) -> Result<Vec<String>> {
        let Some(target_idx) = TAXON_ORDER.iter().position(|l| *l == level) else {
            return Ok(Vec::new());
        };
        let Some(actual) = taxon_level_col(level).and_then(|c| self.actual(c)).map(str::to_string) else {
            return Ok(Vec::new());
        };
        let mut lf = self.scoped(project, collections, deployments);
        lf = self.apply_hide_non_animals(lf, hide_non_animals);
        // Only apply filters for the levels above `level`.
        for (l, values) in taxon_filters {
            let idx = TAXON_ORDER.iter().position(|x| x == l);
            if matches!(idx, Some(i) if i < target_idx) {
                if let Some(col_name) = taxon_level_col(l).and_then(|c| self.actual(c)) {
                    if let Some(expr) = any_of(col_name, values) {
                        lf = lf.filter(expr);
                    }
                }
            }
        }
        let df = lf
            .select([col(&actual).cast(DataType::String).alias("v")])
            .collect()
            .context("failed to scope taxon values")?;
        let scoped = SpeciesData { frame: df, cols: [("v".to_string(), "v".to_string())].into() };
        scoped.unique_strings("v")
    }

    /// Drop non-animals (taglist `rank == "null"`) when requested.
    fn apply_hide_non_animals(&self, lf: LazyFrame, hide: bool) -> LazyFrame {
        if hide {
            if let Some(rank) = self.actual("rank") {
                return lf.filter(col(rank).cast(DataType::String).fill_null(lit("")).neq(lit("null")));
            }
        }
        lf
    }

    /// Per-species counts, filtered. `sort_metric` is "detections" | "captures".
    /// Columns: `species`, `detections`, `captures` (when `event_id` exists), and
    /// the taxonomy fields (`classCN`, `orderCN`, `familyCN`, `genusCN`,
    /// `mazeScientificName`, `rank`) when the taglist columns are present.
    /// `taxon_filters` is a list of (level, values) on `class`/`family`/`genus`/
    /// `species` (AND across levels); `hide_non_animals` drops `rank == "null"`
    /// (the taglist's marker for Blank/Unidentified/etc.).
    pub fn species_stats(
        &self,
        project: &str,
        collections: &[String],
        deployments: &[String],
        sort_metric: &str,
        taxon_filters: &[(String, Vec<String>)],
        hide_non_animals: bool,
    ) -> Result<DataFrame> {
        let species_col = self
            .actual("species")
            .ok_or_else(|| anyhow!("this CSV has no 'species' column"))?
            .to_string();

        let mut lf = self.scoped(project, collections, deployments);

        // Drop null/empty species. Keep every species value the taglist provides
        // (incl. "Blank 无动物", "Human 人"); which to hide is the taxon filter's
        // job below, not something we hardcode.
        let sc = || col(&species_col).cast(DataType::String);
        lf = lf.filter(sc().is_not_null().and(sc().neq(lit(""))));

        // Taxon filter: hide non-animals (rank == "null") and keep only the
        // selected class/family/genus/species (AND across levels).
        lf = self.apply_hide_non_animals(lf, hide_non_animals);
        for (level, values) in taxon_filters {
            if let Some(col_name) = taxon_level_col(level).and_then(|c| self.actual(c)) {
                if let Some(expr) = any_of(col_name, values) {
                    lf = lf.filter(expr);
                }
            }
        }

        let mut aggs = vec![len().alias("detections")];
        if let Some(event) = self.actual("event_id") {
            aggs.push(col(event).n_unique().alias("captures"));
        }
        // Taxonomy is functionally dependent on species — carry it via first().
        for (canonical, out) in [
            ("classcn", "classCN"),
            ("ordercn", "orderCN"),
            ("familycn", "familyCN"),
            ("genuscn", "genusCN"),
            ("mazescientificname", "mazeScientificName"),
            ("rank", "rank"),
        ] {
            if let Some(actual) = self.actual(canonical) {
                aggs.push(col(actual).cast(DataType::String).first().alias(out));
            }
        }
        lf = lf.group_by([sc().alias("species")]).agg(aggs);

        let by = if sort_metric == "captures" && self.has_event_id() { "captures" } else { "detections" };
        lf = lf.sort_by_exprs(
            [col(by)],
            SortMultipleOptions::default().with_order_descending(true),
        );
        lf.collect().context("species aggregation failed")
    }

    /// Apply the project + collection + deployment filters (shared scope).
    fn scoped(&self, project: &str, collections: &[String], deployments: &[String]) -> LazyFrame {
        let mut lf = self.frame.clone().lazy();
        if project != PROJECT_ALL {
            if let Some(name) = self.actual("project") {
                lf = lf.filter(col(name).cast(DataType::String).eq(lit(project.to_string())));
            }
        }
        if let Some(name) = self.actual("collection") {
            if let Some(expr) = any_of(name, collections) { lf = lf.filter(expr); }
        }
        if let Some(name) = self.actual("deployment") {
            if let Some(expr) = any_of(name, deployments) { lf = lf.filter(expr); }
        }
        lf
    }

    /// A CSV of the rows for one species under the current filters — fed straight
    /// back through the time-analysis pipeline so a selected species can reuse
    /// the existing overview/detail/hour charts. All original columns are kept.
    /// A CSV of all rows under a project/collection/deployment scope (no species
    /// filter) — fed back through the time-analysis pipeline so the heavy
    /// deployment charts render only the selected scope, not the whole dataset.
    pub fn scoped_csv(
        &self,
        project: &str,
        collections: &[String],
        deployments: &[String],
    ) -> Result<String> {
        let mut df = self
            .scoped(project, collections, deployments)
            .collect()
            .context("failed to scope rows")?;
        if df.height() == 0 {
            bail!("no rows under the current scope");
        }
        let mut buffer = Vec::new();
        CsvWriter::new(&mut buffer).finish(&mut df).context("failed to serialize scoped subset")?;
        String::from_utf8(buffer).context("scoped subset was not valid UTF-8")
    }

    pub fn filtered_csv(
        &self,
        project: &str,
        collections: &[String],
        deployments: &[String],
        species: &str,
    ) -> Result<String> {
        let species_col = self
            .actual("species")
            .ok_or_else(|| anyhow!("this CSV has no 'species' column"))?
            .to_string();
        let lf = self
            .scoped(project, collections, deployments)
            .filter(col(&species_col).cast(DataType::String).eq(lit(species.to_string())));
        let mut df = lf.collect().context("failed to filter rows for species")?;
        if df.height() == 0 {
            bail!("no rows for species '{species}' under the current filters");
        }
        let mut buffer = Vec::new();
        CsvWriter::new(&mut buffer).finish(&mut df).context("failed to serialize species subset")?;
        String::from_utf8(buffer).context("species subset was not valid UTF-8")
    }

    pub fn has_map_columns(&self) -> bool {
        self.actual("latitude").is_some() && self.actual("longitude").is_some()
    }

    /// Per-deployment rows for the spatial RAI map: deployment, lat, lon,
    /// camera_days, and the selected species' event count. All deployments in
    /// scope are returned (events = 0 where the species is absent) so they still
    /// contribute camera-days to a grid cell's RAI. Requires latitude/longitude
    /// columns; camera_days is optional (null when absent).
    pub fn deployment_points(
        &self,
        project: &str,
        collections: &[String],
        deployments: &[String],
        species: &str,
    ) -> Result<DataFrame> {
        let dep = self.actual("deployment").ok_or_else(|| anyhow!("no 'deployment' column"))?.to_string();
        let lat = self.actual("latitude").ok_or_else(|| anyhow!("no 'latitude' column"))?.to_string();
        let lon = self.actual("longitude").ok_or_else(|| anyhow!("no 'longitude' column"))?.to_string();
        let species_col = self.actual("species").ok_or_else(|| anyhow!("no 'species' column"))?.to_string();

        // Every deployment in scope + its coordinates and camera days.
        let mut aggs = vec![
            col(&lat).cast(DataType::Float64).mean().alias("lat"),
            col(&lon).cast(DataType::Float64).mean().alias("lon"),
        ];
        aggs.push(match self.actual("camera_days") {
            Some(days) => col(days).cast(DataType::Float64).max().alias("camera_days"),
            None => lit(NULL).cast(DataType::Float64).alias("camera_days"),
        });
        let deps = self
            .scoped(project, collections, deployments)
            .group_by([col(&dep).cast(DataType::String).alias("deployment")])
            .agg(aggs);

        // Species event (or detection) count per deployment.
        let event_expr = match self.actual("event_id") {
            Some(event) => col(event).n_unique().alias("events"),
            None => len().alias("events"),
        };
        let events = self
            .scoped(project, collections, deployments)
            .filter(col(&species_col).cast(DataType::String).eq(lit(species.to_string())))
            .group_by([col(&dep).cast(DataType::String).alias("deployment")])
            .agg([event_expr]);

        deps.join(events, [col("deployment")], [col("deployment")], JoinArgs::new(JoinType::Left))
            .with_column(col("events").fill_null(lit(0)).cast(DataType::Int64))
            .collect()
            .context("failed to build deployment points")
    }

    /// Overall activity-by-hour for one species: a 24-row table (`hour`,
    /// `hour_label`, `event_count`, `deployment="activity"`). The value is the
    /// number of **independent events** (distinct `event_id`) whose first
    /// detection falls in that hour — not media rows — so a burst of frames for
    /// one animal counts once. Without an `event_id` column it falls back to
    /// media rows per hour. Hours with no activity are kept (zero-filled).
    pub fn activity_by_hour(
        &self,
        project: &str,
        collections: &[String],
        deployments: &[String],
        species: &str,
    ) -> Result<DataFrame> {
        let dt_col = self.actual("datetime").ok_or_else(|| anyhow!("no 'datetime' column"))?.to_string();
        let species_col = self.actual("species").ok_or_else(|| anyhow!("no 'species' column"))?.to_string();

        let lf = self
            .scoped(project, collections, deployments)
            .filter(col(&species_col).cast(DataType::String).eq(lit(species.to_string())));

        let mut sel = vec![col(&dt_col).cast(DataType::String).alias("__dt")];
        if let Some(event) = self.actual("event_id") {
            sel.push(col(event).cast(DataType::String).alias("__ev"));
        }
        let df = lf.select(sel).collect().context("failed to gather activity rows")?;

        let mut counts = [0i64; 24];
        let dt = df.column("__dt")?.str()?;
        if self.has_event_id() {
            // Assign each distinct event to the hour of its earliest detection.
            // ISO-ish datetime strings sort lexically = chronologically.
            let ev = df.column("__ev")?.str()?;
            let mut earliest: HashMap<String, (String, u32)> = HashMap::new();
            for (d, e) in dt.into_iter().zip(ev.into_iter()) {
                let (Some(d), Some(e)) = (d, e) else { continue };
                let Some(h) = hour_of(d) else { continue };
                earliest
                    .entry(e.to_string())
                    .and_modify(|cur| if d < cur.0.as_str() { *cur = (d.to_string(), h); })
                    .or_insert_with(|| (d.to_string(), h));
            }
            for (_, (_, h)) in earliest {
                counts[h as usize] += 1;
            }
        } else {
            for d in dt.into_iter().flatten() {
                if let Some(h) = hour_of(d) {
                    counts[h as usize] += 1;
                }
            }
        }

        let hours: Vec<i32> = (0..24).collect();
        let labels: Vec<String> = (0..24).map(|h| format!("{h:02}:00")).collect();
        let events: Vec<i64> = counts.to_vec();
        let deployment: Vec<&str> = vec!["activity"; 24];
        df!(
            "hour" => hours,
            "hour_label" => labels,
            "event_count" => events,
            "deployment" => deployment,
        )
        .context("failed to build activity-by-hour table")
    }
}

/// Hour-of-day (0..23) from an ISO-ish datetime string ("YYYY-MM-DD HH:MM:SS"
/// or with a `T` separator), by reading the two digits after the date/time
/// separator. Returns `None` if the shape is unexpected.
fn hour_of(s: &str) -> Option<u32> {
    let sep = s.find(|c| c == ' ' || c == 'T')?;
    s.get(sep + 1..sep + 3)?.parse::<u32>().ok().filter(|h| *h < 24)
}

/// Taxon levels in hierarchy order (drives the cascading dropdowns).
const TAXON_ORDER: [&str; 5] = ["class", "order", "family", "genus", "species"];

/// Level key → canonical (lowercased) column name.
fn taxon_level_col(level: &str) -> Option<&'static str> {
    match level {
        "class" => Some("classcn"),
        "order" => Some("ordercn"),
        "family" => Some("familycn"),
        "genus" => Some("genuscn"),
        "species" => Some("species"),
        _ => None,
    }
}

/// `col == v0 OR col == v1 ...` (values cast to string). None if no values.
fn any_of(col_name: &str, values: &[String]) -> Option<Expr> {
    let values: Vec<&String> = values.iter().filter(|value| !value.is_empty()).collect();
    let (first, rest) = values.split_first()?;
    let c = || col(col_name).cast(DataType::String);
    let mut expr = c().eq(lit((*first).clone()));
    for value in rest {
        expr = expr.or(c().eq(lit((*value).clone())));
    }
    Some(expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "\
project,collection,deployment,species,event_id,path,datetime
maze,c1,dep1,岩羊,e1,a/1.jpg,2026-01-01 08:00:00
maze,c1,dep1,岩羊,e1,a/2.jpg,2026-01-01 08:00:05
maze,c1,dep1,岩羊,e2,a/3.jpg,2026-01-02 09:00:00
maze,c1,dep2,赤狐,e3,b/1.jpg,2026-01-01 20:00:00
maze,c2,dep3,岩羊,e4,c/1.jpg,2026-01-03 07:00:00
maze,c2,dep3,Blank,e5,c/2.jpg,2026-01-03 07:01:00
other,c9,dep9,赤狐,e9,d/1.jpg,2026-01-01 10:00:00
";

    #[test]
    fn aggregates_detections_and_captures() {
        let sd = SpeciesData::from_csv_text(CSV).unwrap();
        assert!(sd.has_species() && sd.has_event_id());
        assert_eq!(sd.projects().unwrap(), vec!["maze".to_string(), "other".to_string()]);

        // project "maze": 岩羊 detections = 4 rows (2x e1 + e2 + e4), captures =
        // {e1,e2,e4} = 3. All species kept (Blank NOT dropped — refining is a UI
        // filter): {岩羊(4), 赤狐(1), Blank(1)}, 岩羊 on top by detections.
        let stats = sd.species_stats("maze", &[], &[], "detections", &[], false).unwrap();
        assert_eq!(stats.height(), 3);
        let species: Vec<&str> = stats.column("species").unwrap().str().unwrap().into_iter().flatten().collect();
        assert_eq!(species[0], "岩羊");
        assert!(species.contains(&"Blank"));
        let det: Vec<u32> = stats.column("detections").unwrap().u32().unwrap().into_iter().flatten().collect();
        assert_eq!(det[0], 4);
        let cap: Vec<u32> = stats.column("captures").unwrap().u32().unwrap().into_iter().flatten().collect();
        assert_eq!(cap[0], 3);
    }

    const CSV_MAP: &str = "\
deployment,species,event_id,latitude,longitude,camera_days
dep1,岩羊,e1,34.5,101.2,30
dep1,岩羊,e2,34.5,101.2,30
dep1,赤狐,e3,34.5,101.2,30
dep2,岩羊,e4,34.6,101.3,20
dep3,赤狐,e5,34.7,101.4,25
";

    #[test]
    fn deployment_points_for_rai() {
        let sd = SpeciesData::from_csv_text(CSV_MAP).unwrap();
        assert!(sd.has_map_columns());
        // 岩羊: dep1 {e1,e2}=2 events, dep2 {e4}=1, dep3 absent -> 0 (but still listed
        // with its camera_days, so it contributes to a cell's RAI denominator).
        let df = sd.deployment_points(PROJECT_ALL, &[], &[], "岩羊").unwrap();
        assert_eq!(df.height(), 3);
        let dep = df.column("deployment").unwrap().str().unwrap();
        let ev = df.column("events").unwrap().i64().unwrap();
        let cd = df.column("camera_days").unwrap().f64().unwrap();
        let lat = df.column("lat").unwrap().f64().unwrap();
        let mut m = std::collections::HashMap::new();
        for i in 0..df.height() {
            m.insert(dep.get(i).unwrap().to_string(), (ev.get(i).unwrap(), cd.get(i).unwrap(), lat.get(i).unwrap()));
        }
        assert_eq!(m["dep1"], (2, 30.0, 34.5));
        assert_eq!(m["dep2"], (1, 20.0, 34.6));
        assert_eq!(m["dep3"], (0, 25.0, 34.7));
    }

    #[test]
    fn filters_by_collection() {
        let sd = SpeciesData::from_csv_text(CSV).unwrap();
        let (collections, deployments) = sd.project_summary("maze").unwrap();
        assert_eq!(collections, vec!["c1".to_string(), "c2".to_string()]);
        assert_eq!(deployments, vec!["dep1".to_string(), "dep2".to_string(), "dep3".to_string()]);
        // only c2 (dep3): 岩羊 1 (e4) + Blank 1 (e8) — both kept now.
        let stats = sd.species_stats("maze", &["c2".to_string()], &[], "detections", &[], false).unwrap();
        assert_eq!(stats.height(), 2);
        let det: Vec<u32> = stats.column("detections").unwrap().u32().unwrap().into_iter().flatten().collect();
        assert_eq!(det, vec![1, 1]);
    }
}
