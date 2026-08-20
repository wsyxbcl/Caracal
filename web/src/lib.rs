#[path = "../../src/data.rs"]
mod data;
#[path = "../../src/render.rs"]
mod render;
#[path = "../../src/species.rs"]
mod species;
#[path = "../../src/util.rs"]
mod util;

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
    data: PreparedData,
    species: SpeciesData, // maze-stats: same CSV, species-analysis view
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
        let override_index = (deploy_path_index >= 1).then_some(deploy_path_index);
        let data = PreparedData::from_csv_text(&csv_content, override_index).map_err(to_js_error)?;
        let species = SpeciesData::from_csv_text(&csv_content).map_err(to_js_error)?;
        Ok(Self { data, species })
    }

    // ---- maze-stats: species analysis --------------------------------------
    /// Whether this CSV carries a `species` column (enables the species tab).
    pub fn species_available(&self) -> bool {
        self.species.has_species()
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

    /// Per-species counts as JSON rows `[{ species, detections, captures? }]`,
    /// filtered + sorted by `sort_metric` ("detections" | "captures").
    pub fn species_stats_json(
        &self,
        project: String,
        collections: String,
        deployments: String,
        sort_metric: String,
    ) -> Result<String, JsValue> {
        let df = self
            .species
            .species_stats(&project, &parse_str_list(&collections)?, &parse_str_list(&deployments)?, &sort_metric)
            .map_err(to_js_error)?;
        species_stats_to_json(&df)
    }

    /// Species bar chart (charton SVG) for the current filter + metric.
    pub fn render_species_bar(
        &self,
        project: String,
        collections: String,
        deployments: String,
        metric: String,
        _theme: String,
    ) -> Result<String, JsValue> {
        let df = self
            .species
            .species_stats(&project, &parse_str_list(&collections)?, &parse_str_list(&deployments)?, &metric)
            .map_err(to_js_error)?;
        render::species_bar_svg(&df, &metric).map_err(to_js_error)
    }

    pub fn metadata_json(&self) -> Result<String, JsValue> {
        let metadata = ExplorerMetadata {
            rows: self.data.events.height(),
            rows_display: format_count(self.data.events.height()),
            deployments: self.data.deployments.len(),
            deployments_display: format_count(self.data.deployments.len()),
            range_start: format_date(self.data.min_timestamp),
            range_end: format_date(self.data.max_timestamp),
            default_bucket: OverviewBucket::Month.slug(),
            default_deployment: self.data.default_deployment().to_string(),
            deployment_options: self
                .data
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
            deployment_from_path: self.data.deployment_source.from_path,
            deploy_path_index: self.data.deployment_source.path_index,
            detected_path_index: self.data.deployment_source.detected_path_index,
            path_levels: self
                .data
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
        render::overview_web_svg(&self.data, bucket, parse_theme(&theme)).map_err(to_js_error)
    }

    pub fn render_detail(&self, deployment: String, theme: String) -> Result<String, JsValue> {
        let deployment = normalize_deployment(&self.data, &deployment)?;
        render::detail_web_svg(&self.data, &deployment, parse_theme(&theme)).map_err(to_js_error)
    }

    pub fn render_hour_heatmap(&self, theme: String) -> Result<String, JsValue> {
        render::hour_heatmap_web_svg(&self.data, parse_theme(&theme)).map_err(to_js_error)
    }

    pub fn detail_caption(&self, deployment: String) -> Result<String, JsValue> {
        let deployment = normalize_deployment(&self.data, &deployment)?;
        render::detail_caption(&self.data, &deployment).map_err(to_js_error)
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

/// Serialize a species-stats DataFrame (`species`, `detections`, optional
/// `captures`) to JSON rows for the table.
fn species_stats_to_json(df: &DataFrame) -> Result<String, JsValue> {
    #[derive(Serialize)]
    struct Row {
        species: String,
        detections: u32,
        captures: Option<u32>,
    }
    let species = df.column("species").map_err(to_js_error)?.str().map_err(to_js_error)?;
    let detections = df.column("detections").map_err(to_js_error)?.u32().map_err(to_js_error)?;
    let captures = df.column("captures").ok().map(|c| c.u32()).transpose().map_err(to_js_error)?;
    let rows: Vec<Row> = (0..df.height())
        .map(|i| Row {
            species: species.get(i).unwrap_or("").to_string(),
            detections: detections.get(i).unwrap_or(0),
            captures: captures.as_ref().and_then(|c| c.get(i)),
        })
        .collect();
    serde_json::to_string(&rows).map_err(to_js_error)
}
