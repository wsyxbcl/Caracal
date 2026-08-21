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

    /// Per-species counts, filtered. `sort_metric` is "detections" | "captures".
    /// Columns: `species`, `detections`, and `captures` when `event_id` exists.
    pub fn species_stats(
        &self,
        project: &str,
        collections: &[String],
        deployments: &[String],
        sort_metric: &str,
    ) -> Result<DataFrame> {
        let species_col = self
            .actual("species")
            .ok_or_else(|| anyhow!("this CSV has no 'species' column"))?
            .to_string();

        let mut lf = self.scoped(project, collections, deployments);

        // Drop non-observations and null/empty species.
        // Keep every species value the taglist provides (incl. "Blank 无动物",
        // "Human 人") — refining what to show is a UI filter later, not something
        // we hardcode here. Only drop rows that have no species value at all.
        let sc = || col(&species_col).cast(DataType::String);
        lf = lf.filter(sc().is_not_null().and(sc().neq(lit(""))));

        let mut aggs = vec![len().alias("detections")];
        if let Some(event) = self.actual("event_id") {
            aggs.push(col(event).n_unique().alias("captures"));
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
        let stats = sd.species_stats("maze", &[], &[], "detections").unwrap();
        assert_eq!(stats.height(), 3);
        let species: Vec<&str> = stats.column("species").unwrap().str().unwrap().into_iter().flatten().collect();
        assert_eq!(species[0], "岩羊");
        assert!(species.contains(&"Blank"));
        let det: Vec<u32> = stats.column("detections").unwrap().u32().unwrap().into_iter().flatten().collect();
        assert_eq!(det[0], 4);
        let cap: Vec<u32> = stats.column("captures").unwrap().u32().unwrap().into_iter().flatten().collect();
        assert_eq!(cap[0], 3);
    }

    #[test]
    fn filters_by_collection() {
        let sd = SpeciesData::from_csv_text(CSV).unwrap();
        let (collections, deployments) = sd.project_summary("maze").unwrap();
        assert_eq!(collections, vec!["c1".to_string(), "c2".to_string()]);
        assert_eq!(deployments, vec!["dep1".to_string(), "dep2".to_string(), "dep3".to_string()]);
        // only c2 (dep3): 岩羊 1 (e4) + Blank 1 (e8) — both kept now.
        let stats = sd.species_stats("maze", &["c2".to_string()], &[], "detections").unwrap();
        assert_eq!(stats.height(), 2);
        let det: Vec<u32> = stats.column("detections").unwrap().u32().unwrap().into_iter().flatten().collect();
        assert_eq!(det, vec![1, 1]);
    }
}
