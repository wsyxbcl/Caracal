#[path = "../../src/data.rs"]
mod data;
#[path = "../../src/render.rs"]
mod render;
#[path = "../../src/species.rs"]
mod species;
#[path = "../../src/util.rs"]
mod util;

use std::cell::{Ref, RefCell};

use polars::prelude::*;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::data::{OverviewBucket, PreparedData};
use crate::species::SpeciesData;
use crate::util::{format_count, format_date, format_timestamp};

#[wasm_bindgen(start)]
pub fn init_runtime() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct WasmExplorer {
    // The time-analysis model is heavy to build (parses every row + aggregates all
    // deployments), so build it lazily: species mode + the light metadata don't
    // need it, and big multi-collection imports only ever render a scoped subset.
    csv: String,
    data: RefCell<Option<PreparedData>>,
    species: SpeciesData, // maze-stats: same CSV, species-analysis view
    deploy_path_index: i32, // remembered so a species-scoped subset derives the same deployments
}

impl WasmExplorer {
    /// Shared constructor: parse the (cheap) species view now; defer the heavy
    /// time-analysis `PreparedData` to first use.
    fn from_csv(csv: String, deploy_path_index: i32) -> Result<WasmExplorer, JsValue> {
        let species = SpeciesData::from_csv_text(&csv).map_err(to_js_error)?;
        Ok(Self { csv, data: RefCell::new(None), species, deploy_path_index })
    }

    /// The time-analysis model, built on first access and cached.
    fn data(&self) -> Result<Ref<'_, PreparedData>, JsValue> {
        let needs_build = self.data.borrow().is_none();
        if needs_build {
            let override_index = (self.deploy_path_index >= 1).then_some(self.deploy_path_index);
            let prepared =
                PreparedData::from_csv_text(&self.csv, override_index).map_err(to_js_error)?;
            *self.data.borrow_mut() = Some(prepared);
        }
        Ok(Ref::map(self.data.borrow(), |slot| slot.as_ref().expect("just built")))
    }
}

#[derive(Serialize)]
struct ExplorerMetadata {
    rows: usize,
    rows_display: String,
    deployments: usize,
    deployments_display: String,
    range_start: String,
    range_end: String,
    default_bucket: &'static str,
    default_deployment: String,
    deployment_options: Vec<DeploymentOption>,
    deployment_from_path: bool,
    deploy_path_index: Option<i32>,
    detected_path_index: Option<i32>,
    path_levels: Vec<PathLevelOption>,
}

#[derive(Serialize)]
struct PathLevelOption {
    index: i32,
    name: String,
}

#[derive(Serialize)]
struct PathLevelProbe {
    has_deployment_column: bool,
    detected_path_index: Option<i32>,
    path_levels: Vec<PathLevelOption>,
}

#[derive(Serialize)]
struct DeploymentOption {
    deployment: String,
    event_count: usize,
    event_count_display: String,
    first_seen: String,
    last_seen: String,
    media_breakdown: String,
    has_gps: bool,
}

#[wasm_bindgen]
impl WasmExplorer {
    /// Build an explorer from CSV text. `deploy_path_index` chooses the
    /// deployment path level for CSVs without a `deployment` column; pass any
    /// value below 1 (e.g. -1) to auto-detect.
    #[wasm_bindgen(constructor)]
    pub fn new(csv_content: String, deploy_path_index: i32) -> Result<WasmExplorer, JsValue> {
        Self::from_csv(csv_content, deploy_path_index)
    }

    // ---- maze-stats: species analysis --------------------------------------
    /// Whether this CSV carries a `species` column (enables the species tab).
    pub fn species_available(&self) -> bool {
        self.species.has_species()
    }

    /// Whether the CSV has latitude/longitude (enables the RAI map).
    pub fn map_available(&self) -> bool {
        self.species.has_map_columns()
    }

    /// Per-deployment points for the RAI map, as JSON rows
    /// `[{ deployment, lat, lon, camera_days, events }]`.
    pub fn deployment_points_json(
        &self,
        project: String,
        collections: String,
        deployments: String,
        species: String,
    ) -> Result<String, JsValue> {
        let df = self
            .species
            .deployment_points(&project, &parse_str_list(&collections)?, &parse_str_list(&deployments)?, &species)
            .map_err(to_js_error)?;
        deployment_points_to_json(&df)
    }

    /// Projects to choose from (JSON array). `["__all__"]` when there's no
    /// `project` column.
    pub fn projects_json(&self) -> Result<String, JsValue> {
        let projects = self.species.projects().map_err(to_js_error)?;
        serde_json::to_string(&projects).map_err(to_js_error)
    }

    /// Collections + deployments within a project, plus whether captures are
    /// available. JSON: `{ collections, deployments, has_event_id }`.
    pub fn project_summary_json(&self, project: String) -> Result<String, JsValue> {
        let (collections, deployments) =
            self.species.project_summary(&project).map_err(to_js_error)?;
        #[derive(Serialize)]
        struct Summary {
            collections: Vec<String>,
            deployments: Vec<String>,
            has_event_id: bool,
        }
        serde_json::to_string(&Summary {
            collections,
            deployments,
            has_event_id: self.species.has_event_id(),
        })
        .map_err(to_js_error)
    }

    /// Whether the CSV carries taxonomy (a `classCN` column).
    pub fn has_taxonomy(&self) -> bool { self.species.has_taxonomy() }

    /// Distinct values for one taxon level (`class`/`family`/`genus`/`species`)
    /// in scope, cascaded by the above-level selections in `taxon_filters_json`.
    /// JSON array.
    pub fn taxonomy_values_json(
        &self,
        level: String,
        project: String,
        collections: String,
        deployments: String,
        taxon_filters_json: String,
        hide_non_animals: bool,
    ) -> Result<String, JsValue> {
        let values = self
            .species
            .taxonomy_values(
                &level,
                &project,
                &parse_str_list(&collections)?,
                &parse_str_list(&deployments)?,
                &parse_taxon_filters(&taxon_filters_json)?,
                hide_non_animals,
            )
            .map_err(to_js_error)?;
        serde_json::to_string(&values).map_err(to_js_error)
    }

    /// Per-species counts as JSON rows `[{ species, detections, captures?, classCN?, ... }]`,
    /// filtered by scope + taxon filter, sorted by `sort_metric`.
    pub fn species_stats_json(
        &self,
        project: String,
        collections: String,
        deployments: String,
        sort_metric: String,
        taxon_filters_json: String,
        hide_non_animals: bool,
    ) -> Result<String, JsValue> {
        let df = self
            .species
            .species_stats(
                &project,
                &parse_str_list(&collections)?,
                &parse_str_list(&deployments)?,
                &sort_metric,
                &parse_taxon_filters(&taxon_filters_json)?,
                hide_non_animals,
            )
            .map_err(to_js_error)?;
        species_stats_to_json(&df)
    }

    /// Species bar chart (charton SVG) for the current filter + metric, split and
    /// coloured by taxonomic class.
    pub fn render_species_bar(
        &self,
        project: String,
        collections: String,
        deployments: String,
        metric: String,
        taxon_filters_json: String,
        hide_non_animals: bool,
        theme: String,
    ) -> Result<String, JsValue> {
        let df = self
            .species
            .species_stats(
                &project,
                &parse_str_list(&collections)?,
                &parse_str_list(&deployments)?,
                &metric,
                &parse_taxon_filters(&taxon_filters_json)?,
                hide_non_animals,
            )
            .map_err(to_js_error)?;
        render::species_bar_svg(&df, &metric, parse_theme(&theme)).map_err(to_js_error)
    }

    /// Overall activity-by-hour (1-D heatmap) for one species under the current
    /// filters — shown in-page on the species tab, updated when a species is
    /// picked.
    pub fn render_species_activity(
        &self,
        project: String,
        collections: String,
        deployments: String,
        species: String,
        theme: String,
    ) -> Result<String, JsValue> {
        let table = self
            .species
            .activity_by_hour(&project, &parse_str_list(&collections)?, &parse_str_list(&deployments)?, &species)
            .map_err(to_js_error)?;
        render::species_activity_web_svg(&table, parse_theme(&theme)).map_err(to_js_error)
    }

    /// A new explorer scoped to one species under the current filters, so the
    /// existing time-analysis views (overview/detail/hour) render for just that
    /// species — "borrow time analysis for free".
    pub fn species_time_explorer(
        &self,
        project: String,
        collections: String,
        deployments: String,
        species: String,
    ) -> Result<WasmExplorer, JsValue> {
        let csv = self
            .species
            .filtered_csv(&project, &parse_str_list(&collections)?, &parse_str_list(&deployments)?, &species)
            .map_err(to_js_error)?;
        Self::from_csv(csv, self.deploy_path_index)
    }

    /// A new explorer scoped to a project + collections (no species filter), so
    /// the time-analysis views render only that scope — the whole-dataset
    /// deployment heatmaps are too large (and not useful) on big multi-collection
    /// imports.
    pub fn scoped_time_explorer(
        &self,
        project: String,
        collections: String,
    ) -> Result<WasmExplorer, JsValue> {
        let csv = self
            .species
            .scoped_csv(&project, &parse_str_list(&collections)?, &[])
            .map_err(to_js_error)?;
        Self::from_csv(csv, self.deploy_path_index)
    }

    /// Whether the CSV carries a `collection` column (drives the time-analysis
    /// scope selector).
    pub fn has_collection(&self) -> bool { self.species.has_collection() }

    /// Full time-analysis metadata (builds `PreparedData`). Used only when the
    /// time charts are actually rendered — `light_metadata_json` covers load.
    pub fn metadata_json(&self) -> Result<String, JsValue> {
        let data = self.data()?;
        let metadata = ExplorerMetadata {
            rows: data.events.height(),
            rows_display: format_count(data.events.height()),
            deployments: data.deployments.len(),
            deployments_display: format_count(data.deployments.len()),
            range_start: format_date(data.min_timestamp),
            range_end: format_date(data.max_timestamp),
            default_bucket: OverviewBucket::Month.slug(),
            default_deployment: data.default_deployment().to_string(),
            deployment_options: data
                .deployments
                .iter()
                .map(|summary| DeploymentOption {
                    deployment: summary.deployment.clone(),
                    event_count: summary.event_count,
                    event_count_display: format_count(summary.event_count),
                    first_seen: format_timestamp(summary.first_seen),
                    last_seen: format_timestamp(summary.last_seen),
                    media_breakdown: summary.media_breakdown(),
                    has_gps: summary.has_gps,
                })
                .collect(),
            deployment_from_path: data.deployment_source.from_path,
            deploy_path_index: data.deployment_source.path_index,
            detected_path_index: data.deployment_source.detected_path_index,
            path_levels: data
                .deployment_source
                .path_levels
                .iter()
                .enumerate()
                .map(|(offset, name)| PathLevelOption {
                    index: offset as i32 + 1,
                    name: name.clone(),
                })
                .collect(),
        };

        serde_json::to_string(&metadata).map_err(to_js_error)
    }

    /// Cheap load-time metadata straight from the species frame — row/deployment
    /// counts + date range + column presence, WITHOUT building `PreparedData`.
    /// Enough for the metrics strip and the lazy-render decision.
    pub fn light_metadata_json(&self) -> Result<String, JsValue> {
        let (range_start, range_end) = self.species.datetime_range();
        let deployments = self.species.deployment_count();
        let rows = self.species.row_count();
        #[derive(Serialize)]
        struct LightMetadata {
            rows: usize,
            rows_display: String,
            deployments: usize,
            deployments_display: String,
            range_start: String,
            range_end: String,
            has_species: bool,
            has_collection: bool,
        }
        serde_json::to_string(&LightMetadata {
            rows,
            rows_display: format_count(rows),
            deployments,
            deployments_display: format_count(deployments),
            range_start,
            range_end,
            has_species: self.species.has_species(),
            has_collection: self.species.has_collection(),
        })
        .map_err(to_js_error)
    }

    /// Probe a CSV's `path` column so the UI can offer manual level selection
    /// when automatic path derivation fails. Returns JSON with
    /// `has_deployment_column`, `detected_path_index`, and
    /// `path_levels: [{ index, name }]`. Errors when there is no usable `path`
    /// column (then the UI should ask for a `deployment` column instead).
    pub fn probe_path_levels(csv_content: String) -> Result<String, JsValue> {
        let probe = PreparedData::probe_deploy_path(&csv_content).map_err(to_js_error)?;
        let payload = PathLevelProbe {
            has_deployment_column: probe.has_deployment_column,
            detected_path_index: probe.detected_path_index,
            path_levels: probe
                .path_levels
                .iter()
                .enumerate()
                .map(|(offset, name)| PathLevelOption {
                    index: offset as i32 + 1,
                    name: name.clone(),
                })
                .collect(),
        };
        serde_json::to_string(&payload).map_err(to_js_error)
    }

    pub fn render_overview(&self, bucket: String, theme: String) -> Result<String, JsValue> {
        let bucket = parse_bucket(&bucket)?;
        let data = self.data()?;
        render::overview_web_svg(&data, bucket, parse_theme(&theme)).map_err(to_js_error)
    }

    pub fn render_detail(&self, deployment: String, theme: String) -> Result<String, JsValue> {
        let data = self.data()?;
        let deployment = normalize_deployment(&data, &deployment)?;
        render::detail_web_svg(&data, &deployment, parse_theme(&theme)).map_err(to_js_error)
    }

    pub fn render_hour_heatmap(&self, theme: String) -> Result<String, JsValue> {
        let data = self.data()?;
        render::hour_heatmap_web_svg(&data, parse_theme(&theme)).map_err(to_js_error)
    }

    pub fn detail_caption(&self, deployment: String) -> Result<String, JsValue> {
        let data = self.data()?;
        let deployment = normalize_deployment(&data, &deployment)?;
        render::detail_caption(&data, &deployment).map_err(to_js_error)
    }
}

fn parse_theme(value: &str) -> render::ChartTheme {
    match value {
        "dark" => render::ChartTheme::Dark,
        _ => render::ChartTheme::Light,
    }
}

fn parse_bucket(value: &str) -> Result<OverviewBucket, JsValue> {
    match value {
        "day" => Ok(OverviewBucket::Day),
        "week" => Ok(OverviewBucket::Week),
        "month" => Ok(OverviewBucket::Month),
        other => Err(JsValue::from_str(&format!(
            "unknown overview bucket: {other}"
        ))),
    }
}

fn normalize_deployment(data: &PreparedData, deployment: &str) -> Result<String, JsValue> {
    let deployment = deployment.trim();
    if deployment.is_empty() {
        return Ok(data.default_deployment().to_string());
    }

    data.deployment_summary(deployment).map_err(to_js_error)?;
    Ok(deployment.to_string())
}

fn to_js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

/// Parse a JSON string array (the UI's multi-select) into `Vec<String>`.
fn parse_str_list(json: &str) -> Result<Vec<String>, JsValue> {
    serde_json::from_str(json).map_err(to_js_error)
}

/// Parse `{"class":[...],"family":[...],...}` into `[(level, values)]`.
fn parse_taxon_filters(json: &str) -> Result<Vec<(String, Vec<String>)>, JsValue> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let map: std::collections::HashMap<String, Vec<String>> =
        serde_json::from_str(json).map_err(to_js_error)?;
    Ok(map.into_iter().collect())
}

/// Serialize a species-stats DataFrame (`species`, `detections`, optional
/// `captures`) to JSON rows for the table.
fn species_stats_to_json(df: &DataFrame) -> Result<String, JsValue> {
    #[derive(Serialize)]
    struct Row {
        species: String,
        detections: u32,
        captures: Option<u32>,
        #[serde(rename = "classCN", skip_serializing_if = "Option::is_none")]
        class_cn: Option<String>,
        #[serde(rename = "orderCN", skip_serializing_if = "Option::is_none")]
        order_cn: Option<String>,
        #[serde(rename = "familyCN", skip_serializing_if = "Option::is_none")]
        family_cn: Option<String>,
        #[serde(rename = "genusCN", skip_serializing_if = "Option::is_none")]
        genus_cn: Option<String>,
        #[serde(rename = "scientificName", skip_serializing_if = "Option::is_none")]
        scientific_name: Option<String>,
    }
    let species = df.column("species").map_err(to_js_error)?.str().map_err(to_js_error)?;
    let detections = df.column("detections").map_err(to_js_error)?.u32().map_err(to_js_error)?;
    let captures = df.column("captures").ok().map(|c| c.u32()).transpose().map_err(to_js_error)?;
    // Optional taxonomy string columns.
    let str_col = |name: &str| df.column(name).ok().and_then(|c| c.str().ok().cloned());
    let class_cn = str_col("classCN");
    let order_cn = str_col("orderCN");
    let family_cn = str_col("familyCN");
    let genus_cn = str_col("genusCN");
    let scientific_name = str_col("mazeScientificName");
    let opt = |c: &Option<polars::prelude::StringChunked>, i: usize| {
        c.as_ref().and_then(|s| s.get(i)).filter(|v| !v.is_empty()).map(str::to_string)
    };
    let rows: Vec<Row> = (0..df.height())
        .map(|i| Row {
            species: species.get(i).unwrap_or("").to_string(),
            detections: detections.get(i).unwrap_or(0),
            captures: captures.as_ref().and_then(|c| c.get(i)),
            class_cn: opt(&class_cn, i),
            order_cn: opt(&order_cn, i),
            family_cn: opt(&family_cn, i),
            genus_cn: opt(&genus_cn, i),
            scientific_name: opt(&scientific_name, i),
        })
        .collect();
    serde_json::to_string(&rows).map_err(to_js_error)
}

/// Serialize deployment points (deployment, lat, lon, camera_days, events) to JSON.
fn deployment_points_to_json(df: &DataFrame) -> Result<String, JsValue> {
    #[derive(Serialize)]
    struct Point {
        deployment: String,
        lat: Option<f64>,
        lon: Option<f64>,
        camera_days: Option<f64>,
        events: i64,
    }
    let deployment = df.column("deployment").map_err(to_js_error)?.str().map_err(to_js_error)?;
    let lat = df.column("lat").map_err(to_js_error)?.f64().map_err(to_js_error)?;
    let lon = df.column("lon").map_err(to_js_error)?.f64().map_err(to_js_error)?;
    let camera_days = df.column("camera_days").map_err(to_js_error)?.f64().map_err(to_js_error)?;
    let events = df.column("events").map_err(to_js_error)?.i64().map_err(to_js_error)?;
    let rows: Vec<Point> = (0..df.height())
        .map(|i| Point {
            deployment: deployment.get(i).unwrap_or("").to_string(),
            lat: lat.get(i),
            lon: lon.get(i),
            camera_days: camera_days.get(i),
            events: events.get(i).unwrap_or(0),
        })
        .collect();
    serde_json::to_string(&rows).map_err(to_js_error)
}
