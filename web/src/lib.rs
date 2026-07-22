#[path = "../../src/data.rs"]
mod data;
#[path = "../../src/render.rs"]
mod render;
#[path = "../../src/util.rs"]
mod util;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::data::{OverviewBucket, PreparedData};
use crate::util::{format_count, format_date, format_timestamp};

#[wasm_bindgen(start)]
pub fn init_runtime() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct WasmExplorer {
    data: PreparedData,
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
        Ok(Self { data })
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

    pub fn render_overview(&self, bucket: String) -> Result<String, JsValue> {
        let bucket = parse_bucket(&bucket)?;
        render::overview_web_svg(&self.data, bucket).map_err(to_js_error)
    }

    pub fn render_detail(&self, deployment: String) -> Result<String, JsValue> {
        let deployment = normalize_deployment(&self.data, &deployment)?;
        render::detail_web_svg(&self.data, &deployment).map_err(to_js_error)
    }

    pub fn render_hour_heatmap(&self) -> Result<String, JsValue> {
        render::hour_heatmap_web_svg(&self.data).map_err(to_js_error)
    }

    pub fn detail_caption(&self, deployment: String) -> Result<String, JsValue> {
        let deployment = normalize_deployment(&self.data, &deployment)?;
        render::detail_caption(&self.data, &deployment).map_err(to_js_error)
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
