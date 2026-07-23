use anyhow::{Context, Result, bail};
use charton::alt::{color, shape, x, y};
use charton::prelude::*;
use chrono::DateTime;
use polars::prelude::{DataFrame, DataType, Series};

use crate::data::{DeploymentSummary, OverviewBucket, PreparedData};
use crate::util::{escape_html, format_count, format_date};

fn chart_from_table(table: &DataFrame) -> Result<Chart> {
    let mut dataset = Dataset::new();
    for column in table.get_columns() {
        add_series_to_dataset(&mut dataset, column.as_materialized_series())?;
    }
    Ok(Chart::build(dataset)?)
}

fn add_series_to_dataset(dataset: &mut Dataset, series: &Series) -> Result<()> {
    let name = series.name().to_string();
    match series.dtype() {
        DataType::Float64 => {
            let values = series
                .f64()
                .with_context(|| format!("failed to read column '{name}' as Float64"))?
                .into_iter()
                .collect::<Vec<Option<f64>>>();
            dataset.add_column(name, values)?;
        }
        DataType::Float32 => {
            let values = series
                .f32()
                .with_context(|| format!("failed to read column '{name}' as Float32"))?
                .into_iter()
                .collect::<Vec<Option<f32>>>();
            dataset.add_column(name, values)?;
        }
        DataType::Int64 => {
            let values = series
                .i64()
                .with_context(|| format!("failed to read column '{name}' as Int64"))?
                .into_iter()
                .collect::<Vec<Option<i64>>>();
            dataset.add_column(name, values)?;
        }
        DataType::Int32 => {
            let values = series
                .i32()
                .with_context(|| format!("failed to read column '{name}' as Int32"))?
                .into_iter()
                .collect::<Vec<Option<i32>>>();
            dataset.add_column(name, values)?;
        }
        DataType::Int16 => {
            let values = series
                .i16()
                .with_context(|| format!("failed to read column '{name}' as Int16"))?
                .into_iter()
                .collect::<Vec<Option<i16>>>();
            dataset.add_column(name, values)?;
        }
        DataType::Int8 => {
            let values = series
                .i8()
                .with_context(|| format!("failed to read column '{name}' as Int8"))?
                .into_iter()
                .collect::<Vec<Option<i8>>>();
            dataset.add_column(name, values)?;
        }
        DataType::UInt64 => {
            let values = series
                .u64()
                .with_context(|| format!("failed to read column '{name}' as UInt64"))?
                .into_iter()
                .collect::<Vec<Option<u64>>>();
            dataset.add_column(name, values)?;
        }
        DataType::UInt32 => {
            let values = series
                .u32()
                .with_context(|| format!("failed to read column '{name}' as UInt32"))?
                .into_iter()
                .collect::<Vec<Option<u32>>>();
            dataset.add_column(name, values)?;
        }
        DataType::String => {
            let values = series
                .str()
                .with_context(|| format!("failed to read column '{name}' as String"))?
                .into_iter()
                .map(|value| value.map(ToOwned::to_owned))
                .collect::<Vec<Option<String>>>();
            dataset.add_column(name, values)?;
        }
        DataType::Boolean => {
            let values = series
                .bool()
                .with_context(|| format!("failed to read column '{name}' as Boolean"))?
                .into_iter()
                .collect::<Vec<Option<bool>>>();
            dataset.add_column(name, values)?;
        }
        other => bail!("unsupported chart column '{name}' with dtype {other}"),
    }

    Ok(())
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn overview_chart(data: &PreparedData, bucket: OverviewBucket) -> Result<LayeredChart> {
    overview_chart_from_table(
        data.overview_table(bucket)?,
        bucket,
        &format!("Deployment × {} heatmap", bucket.display_name()),
    )
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn overview_web_chart(
    data: &PreparedData,
    bucket: OverviewBucket,
    palette: &WebPalette,
) -> Result<LayeredChart> {
    overview_web_chart_from_table(data.overview_table(bucket)?, bucket, palette)
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn overview_chart_from_table(
    table: &DataFrame,
    bucket: OverviewBucket,
    title: &str,
) -> Result<LayeredChart> {
    let chart = chart_from_table(table)?
        .mark_rect()?
        .encode((x("bucket_label"), y("deployment"), color("event_count")))?
        .configure_rect(|rect| rect.with_stroke("#f7f1e7").with_stroke_width(0.2))
        .with_size(1860, 920)
        .with_title(title)
        .with_x_label(bucket.axis_label())
        .with_y_label("Deployment")
        .configure_theme(|_| {
            base_theme()
                .with_color_map(ColorMap::YlOrRd)
                .with_tick_label_size(11.0)
                .with_x_tick_label_angle(-42.0)
                .with_axis_reserve_buffer(10.0)
                .with_tick_min_spacing(80.0)
                .with_panel_defense_ratio(0.28)
                .with_title_size(22.0)
                .with_legend_title_size(13.0)
                .with_legend_label_size(11.0)
                .with_left_margin(0.13)
                .with_right_margin(0.05)
                .with_bottom_margin(0.16)
                .with_top_margin(0.11)
        });

    Ok(chart)
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn overview_web_chart_from_table(
    table: &DataFrame,
    _bucket: OverviewBucket,
    palette: &WebPalette,
) -> Result<LayeredChart> {
    let chart = chart_from_table(table)?
        .mark_rect()?
        .encode((x("bucket_label"), y("deployment"), color("event_count")))?
        .configure_rect(|rect| rect.with_stroke(palette.rect_stroke).with_stroke_width(0.2))
        .with_size(1320, 760)
        .with_x_label("")
        .with_y_label("")
        .configure_theme(|_| {
            web_theme(palette)
                .with_color_map(palette.overview_map)
                .with_tick_label_size(11.0)
                .with_x_tick_label_angle(-42.0)
                .with_axis_reserve_buffer(3.0)
                .with_tick_min_spacing(80.0)
                .with_panel_defense_ratio(0.14)
        })
        .with_margins(0.018, 0.016, 0.03, 0.008);
    Ok(chart)
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn detail_chart(data: &PreparedData, deployment: &str) -> Result<LayeredChart> {
    detail_chart_from_table(
        data.detail_table(deployment)?,
        deployment,
        data.deployment_summary(deployment)?,
    )
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn detail_web_chart(
    data: &PreparedData,
    deployment: &str,
    palette: &WebPalette,
) -> Result<LayeredChart> {
    detail_web_chart_from_table(
        data.detail_table(deployment)?,
        deployment,
        data.deployment_summary(deployment)?,
        palette,
    )
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn detail_chart_from_table(
    table: &DataFrame,
    _deployment: &str,
    summary: &DeploymentSummary,
) -> Result<LayeredChart> {
    let chart = chart_from_table(table)?
        .mark_point()?
        .encode((
            x("day_label"),
            y("minute_of_day"),
            color("media_family"),
            shape("media_family"),
        ))?
        .configure_point(|point| {
            point
                .with_size(2.9)
                .with_opacity(0.82)
                .with_stroke("#fffaf2")
                .with_stroke_width(0.28)
        })
        .with_size(1640, 560)
        .with_title(format!("Deployment detail: {}", summary.deployment))
        .with_x_label("Day")
        .with_y_label("Time of day")
        .with_y_domain(0.0, 1440.0)
        .configure_theme(|_| {
            base_theme()
                .with_palette(ColorPalette::Dark2)
                .with_x_tick_label_angle(-40.0)
                .with_tick_label_size(11.0)
                .with_tick_min_spacing(62.0)
                .with_title_size(22.0)
                .with_right_margin(0.08)
                .with_left_margin(0.08)
                .with_bottom_margin(0.18)
                .with_top_margin(0.12)
                .with_grid_color("#e3d7c7")
        });

    Ok(chart)
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn detail_web_chart_from_table(
    table: &DataFrame,
    _deployment: &str,
    _summary: &DeploymentSummary,
    palette: &WebPalette,
) -> Result<LayeredChart> {
    let chart = chart_from_table(table)?
        .mark_point()?
        .encode((
            x("day_label"),
            y("minute_of_day"),
            color("media_family"),
            shape("media_family"),
        ))?
        .configure_point(|point| {
            point
                .with_size(2.8)
                .with_opacity(0.84)
                .with_stroke(palette.point_stroke)
                .with_stroke_width(0.30)
        })
        .with_size(1280, 420)
        .with_x_label("")
        .with_y_label("")
        .with_y_domain(0.0, 1440.0)
        .configure_theme(|_| {
            web_theme(palette)
                .with_palette(ColorPalette::Dark2)
                .with_x_tick_label_angle(-40.0)
                .with_tick_label_size(11.0)
                .with_tick_min_spacing(52.0)
                .with_right_margin(0.03)
                .with_left_margin(0.04)
                .with_bottom_margin(0.11)
                .with_top_margin(0.02)
                .with_grid_color(palette.detail_grid)
        });

    Ok(chart)
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn hour_heatmap_chart(data: &PreparedData) -> Result<LayeredChart> {
    hour_heatmap_chart_from_table(&data.hour_heatmap, "Hour-of-day activity by deployment")
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn hour_heatmap_web_chart(data: &PreparedData, palette: &WebPalette) -> Result<LayeredChart> {
    hour_heatmap_web_chart_from_table(&data.hour_heatmap, palette)
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn hour_heatmap_chart_from_table(table: &DataFrame, title: &str) -> Result<LayeredChart> {
    let chart = chart_from_table(table)?
        .mark_rect()?
        .encode((x("hour_label"), y("deployment"), color("event_count")))?
        .configure_rect(|rect| rect.with_stroke("#f7f1e7").with_stroke_width(0.28))
        .with_size(1160, 920)
        .with_title(title)
        .with_x_label("Hour of day")
        .with_y_label("Deployment")
        .configure_theme(|_| {
            base_theme()
                .with_color_map(ColorMap::GnBu)
                .with_tick_label_size(11.0)
                .with_tick_min_spacing(40.0)
                .with_left_margin(0.14)
                .with_bottom_margin(0.12)
                .with_top_margin(0.11)
                .with_right_margin(0.08)
        });

    Ok(chart)
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn hour_heatmap_web_chart_from_table(
    table: &DataFrame,
    palette: &WebPalette,
) -> Result<LayeredChart> {
    let chart = chart_from_table(table)?
        .mark_rect()?
        .encode((x("hour_label"), y("deployment"), color("event_count")))?
        .configure_rect(|rect| rect.with_stroke(palette.rect_stroke).with_stroke_width(0.28))
        .with_size(980, 760)
        .with_x_label("")
        .with_y_label("")
        .configure_theme(|_| {
            web_theme(palette)
                .with_color_map(palette.hour_map)
                .with_tick_label_size(11.0)
                .with_tick_min_spacing(40.0)
        })
        .with_margins(0.018, 0.02, 0.02, 0.008);

    Ok(chart)
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn overview_svg(data: &PreparedData, bucket: OverviewBucket) -> Result<String> {
    let svg = overview_chart(data, bucket)?.to_svg()?;
    let svg = relabel_legend_titles(&svg, &[("event_count", "media_count")]);
    let svg = thin_rotated_bottom_axis_labels(&svg, target_overview_label_count(bucket));
    let svg = annotate_overview_svg(&svg, data.overview_table(bucket)?)?;
    let svg = force_zero_event_cells(&svg, HEATMAP_ZERO_FILL_WHITE);
    Ok(force_zero_legend_stop(&svg, HEATMAP_ZERO_FILL_WHITE))
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn overview_web_svg(
    data: &PreparedData,
    bucket: OverviewBucket,
    theme: ChartTheme,
) -> Result<String> {
    let palette = WebPalette::from_theme(theme);
    let svg = overview_web_chart(data, bucket, &palette)?.to_svg()?;
    let svg = relabel_legend_titles(&svg, &[("event_count", "media_count")]);
    let svg = thin_rotated_bottom_axis_labels(&svg, target_overview_label_count(bucket));
    let svg = strip_svg_background(&svg);
    let svg = annotate_overview_svg(&svg, data.overview_table(bucket)?)?;
    let svg = force_zero_event_cells(&svg, palette.blank_fill);
    Ok(force_zero_legend_stop(&svg, palette.blank_fill))
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn detail_svg(data: &PreparedData, deployment: &str) -> Result<String> {
    let table = data.detail_table(deployment)?;
    let svg = detail_chart(data, deployment)?.to_svg()?;
    let svg = relabel_legend_titles(&svg, &[("media_family", "media")]);
    let svg = thin_rotated_bottom_axis_labels(&svg, target_detail_day_label_count(false));
    let svg = rewrite_detail_minute_axis(&svg, "rgba(38,64,61,1.000)")?;
    let svg = offset_detail_mark_positions(
        &svg,
        table,
        DETAIL_MEDIA_OFFSET_X_PX,
        DETAIL_MEDIA_OFFSET_Y_PX,
    )?;
    annotate_detail_svg(&svg, table)
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn detail_web_svg(
    data: &PreparedData,
    deployment: &str,
    theme: ChartTheme,
) -> Result<String> {
    let palette = WebPalette::from_theme(theme);
    let table = data.detail_table(deployment)?;
    let svg = detail_web_chart(data, deployment, &palette)?.to_svg()?;
    let svg = relabel_legend_titles(&svg, &[("media_family", "media")]);
    let svg = thin_rotated_bottom_axis_labels(&svg, target_detail_day_label_count(true));
    let svg = rewrite_detail_minute_axis(&svg, palette.axis_ink)?;
    let svg = offset_detail_mark_positions(
        &svg,
        table,
        DETAIL_MEDIA_OFFSET_X_PX,
        DETAIL_MEDIA_OFFSET_Y_PX,
    )?;
    let svg = strip_svg_background(&svg);
    annotate_detail_svg(&svg, table)
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn hour_heatmap_svg(data: &PreparedData) -> Result<String> {
    let svg = hour_heatmap_chart(data)?.to_svg()?;
    let svg = relabel_legend_titles(&svg, &[("event_count", "media_count")]);
    let svg = annotate_hour_heatmap_svg(&svg, &data.hour_heatmap)?;
    let svg = boost_low_positive_event_cells(&svg, 3, HEATMAP_LOW_POSITIVE_FILL_GN_BU);
    let svg = force_zero_event_cells(&svg, HEATMAP_ZERO_FILL_WHITE);
    Ok(force_zero_legend_stop(&svg, HEATMAP_ZERO_FILL_WHITE))
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn hour_heatmap_web_svg(data: &PreparedData, theme: ChartTheme) -> Result<String> {
    let palette = WebPalette::from_theme(theme);
    let svg = hour_heatmap_web_chart(data, &palette)?.to_svg()?;
    let svg = relabel_legend_titles(&svg, &[("event_count", "media_count")]);
    let svg = strip_svg_background(&svg);
    let svg = annotate_hour_heatmap_svg(&svg, &data.hour_heatmap)?;
    let svg = match palette.low_positive_fill {
        Some(fill) => boost_low_positive_event_cells(&svg, 3, fill),
        None => svg,
    };
    let svg = force_zero_event_cells(&svg, palette.blank_fill);
    Ok(force_zero_legend_stop(&svg, palette.blank_fill))
}

pub fn detail_caption(data: &PreparedData, deployment: &str) -> Result<String> {
    let summary = data.deployment_summary(deployment)?;
    Ok(detail_caption_from_summary(summary))
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn resolve_deployment<'a>(
    data: &'a PreparedData,
    requested: Option<&'a str>,
) -> Result<&'a str> {
    match requested {
        Some(value) => {
            data.deployment_summary(value)?;
            Ok(value)
        }
        None => Ok(data.default_deployment()),
    }
}

pub fn detail_caption_from_summary(summary: &DeploymentSummary) -> String {
    format!(
        "{} media items from {} through {}. Media types: {}.",
        format_count(summary.event_count),
        format_date(summary.first_seen),
        format_date(summary.last_seen),
        summary.media_breakdown()
    )
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn base_theme() -> Theme {
    Theme::default()
        .with_background_color("#fffaf2")
        .with_grid_color("#e7dccd")
        .with_grid_width(0.8)
        .with_title_family(display_font())
        .with_label_family(sans_font())
        .with_tick_label_family(sans_font())
        .with_legend_label_family(sans_font())
        .with_title_color("#12211f")
        .with_label_color("#26403d")
        .with_tick_label_color("#26403d")
        .with_legend_label_color("#26403d")
        .with_palette(ColorPalette::Dark2)
        .with_show_axes(true)
        .with_axis_width(1.0)
        .with_tick_length(5.0)
        .with_label_size(14.0)
        .with_title_size(20.0)
}

/// Light or dark rendering for the browser charts. Native/CLI export is
/// always light.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Clone, Copy)]
pub enum ChartTheme {
    Light,
    Dark,
}

/// Theme-dependent colors for the web charts. The chart background stays
/// transparent in both themes so the surrounding panel shows through.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub struct WebPalette {
    grid: &'static str,
    detail_grid: &'static str,
    title: &'static str,
    text: &'static str,
    rect_stroke: &'static str,
    point_stroke: &'static str,
    /// Zero heatmap cells and the legend's zero stop — matches the panel so
    /// blanks blend in.
    blank_fill: &'static str,
    /// Injected minute-axis line/label color.
    axis_ink: &'static str,
    /// Heatmap colormaps. Light-mode maps run pale (low) -> dark (high); dark
    /// mode uses perceptual maps that run near-black (low) -> bright (high) so
    /// contrast still grows with value against the dark panel.
    overview_map: ColorMap,
    hour_map: ColorMap,
    /// Fill for boosting barely-positive hour cells so they lift off a light
    /// background; `None` in dark mode, where low values should stay dark.
    low_positive_fill: Option<&'static str>,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl WebPalette {
    fn from_theme(theme: ChartTheme) -> Self {
        match theme {
            ChartTheme::Light => Self {
                grid: "#e7dccd",
                detail_grid: "#e3d7c7",
                title: "#12211f",
                text: "#26403d",
                rect_stroke: "#f7f1e7",
                point_stroke: "#fffaf2",
                blank_fill: "rgba(255,255,255,1.000)",
                axis_ink: "rgba(38,64,61,1.000)",
                overview_map: ColorMap::YlOrRd,
                hour_map: ColorMap::GnBu,
                low_positive_fill: Some(HEATMAP_LOW_POSITIVE_FILL_GN_BU),
            },
            ChartTheme::Dark => Self {
                grid: "#34343a",
                detail_grid: "#34343a",
                title: "#e8e8ea",
                text: "#c9c9cd",
                rect_stroke: "#202023",
                point_stroke: "#202023",
                blank_fill: "rgba(32,32,35,1.000)",
                axis_ink: "rgba(201,201,205,1.000)",
                overview_map: ColorMap::Inferno,
                hour_map: ColorMap::Viridis,
                low_positive_fill: None,
            },
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn web_theme(palette: &WebPalette) -> Theme {
    Theme::default()
        .with_background_color("rgba(255,255,255,0)")
        .with_grid_color(palette.grid)
        .with_grid_width(0.8)
        .with_title_family(display_font())
        .with_label_family(sans_font())
        .with_tick_label_family(sans_font())
        .with_legend_label_family(sans_font())
        .with_title_color(palette.title)
        .with_label_color(palette.text)
        .with_tick_label_color(palette.text)
        .with_legend_label_color(palette.text)
        .with_palette(ColorPalette::Dark2)
        .with_show_axes(true)
        .with_axis_width(1.0)
        .with_tick_length(3.0)
        .with_tick_label_padding(1.0)
        .with_label_size(13.0)
        .with_title_size(18.0)
}

fn target_overview_label_count(bucket: OverviewBucket) -> usize {
    match bucket {
        OverviewBucket::Day => 14,
        OverviewBucket::Week => 12,
        OverviewBucket::Month => 8,
    }
}

fn target_detail_day_label_count(web: bool) -> usize {
    if web { 9 } else { 12 }
}

const HEATMAP_ZERO_FILL_WHITE: &str = "rgba(255,255,255,1.000)";
const HEATMAP_LOW_POSITIVE_FILL_GN_BU: &str = "rgba(223,242,218,1.000)";
const PLOT_CLIP_GROUP: &str = r#"<g clip-path="url(#plot-clip-area)">"#;
const DETAIL_MEDIA_OFFSET_X_PX: f64 = 3.0;
const DETAIL_MEDIA_OFFSET_Y_PX: f64 = 1.8;

fn thin_rotated_bottom_axis_labels(svg: &str, target_labels: usize) -> String {
    let lines = svg.lines().collect::<Vec<_>>();
    let label_indices = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| is_rotated_bottom_tick_label(line).then_some(index))
        .collect::<Vec<_>>();

    if label_indices.len() <= target_labels.max(2) {
        return svg.to_string();
    }

    let stride = ((label_indices.len() as f64) / (target_labels as f64)).ceil() as usize;
    let last_label_position = label_indices.len().saturating_sub(1);

    let mut skip = vec![false; lines.len()];
    for (position, &line_index) in label_indices.iter().enumerate() {
        let keep = position == 0 || position == last_label_position || position % stride == 0;
        if !keep {
            skip[line_index] = true;
            if line_index > 0 && lines[line_index - 1].trim_start().starts_with("<line ") {
                skip[line_index - 1] = true;
            }
        }
    }

    let mut out = String::with_capacity(svg.len());
    for (index, line) in lines.iter().enumerate() {
        if !skip[index] {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !svg.ends_with('\n') {
        out.pop();
    }

    out
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn strip_svg_background(svg: &str) -> String {
    let Some(start_index) = svg.find(r#"<rect width="100%" height="100%""#) else {
        return svg.to_string();
    };
    let Some(end_relative) = svg[start_index..].find("/>") else {
        return svg.to_string();
    };
    let end_index = start_index + end_relative + 2;
    format!("{}{}", &svg[..start_index], &svg[end_index..])
}

fn rewrite_detail_minute_axis(svg: &str, ink: &str) -> Result<String> {
    let lines = svg.lines().collect::<Vec<_>>();
    let Some((plot_x, plot_y, _plot_width, plot_height)) = parse_plot_clip_rect(svg) else {
        return Ok(svg.to_string());
    };

    let mut skip = vec![false; lines.len()];
    for (index, line) in lines.iter().enumerate() {
        if is_left_tick_label_line(line, plot_x) {
            skip[index] = true;
            if index > 0 && is_left_tick_line(lines[index - 1], plot_x) {
                skip[index - 1] = true;
            }
        }
    }

    let ticks = [0.0_f64, 360.0, 720.0, 1080.0, 1440.0];
    let mut injected = String::new();
    for tick in ticks {
        let y = plot_y + plot_height - (tick / 1440.0) * plot_height;
        let label = format_minute_tick_label(tick);
        injected.push_str(&format!(
            r#"<line x1="{x1:.2}" y1="{y:.2}" x2="{x2:.2}" y2="{y:.2}" stroke="{ink}" stroke-width="1.0"/>"#,
            x1 = plot_x,
            x2 = plot_x - 6.0,
        ));
        injected.push('\n');
        injected.push_str(&format!(
            r#"<text x="{x:.2}" y="{y:.2}" font-size="11" font-family="{font}" fill="{ink}" text-anchor="end" dominant-baseline="central">{label}</text>"#,
            x = plot_x - 10.0,
            font = sans_font(),
            label = escape_html(&label),
        ));
        injected.push('\n');
    }

    let mut output = String::with_capacity(svg.len() + injected.len());
    let mut inserted = false;
    for (index, line) in lines.iter().enumerate() {
        if skip[index] {
            continue;
        }
        output.push_str(line);
        output.push('\n');
        if !inserted && line.contains("</defs>") {
            output.push_str(&injected);
            inserted = true;
        }
    }

    if !inserted {
        output.push_str(&injected);
    }

    if !svg.ends_with('\n') {
        output.pop();
    }

    Ok(output)
}

fn parse_plot_clip_rect(svg: &str) -> Option<(f64, f64, f64, f64)> {
    let marker = r#"clipPath id="plot-clip-area"><rect "#;
    let start = svg.find(marker)? + marker.len();
    let end = svg[start..].find("/>")? + start;
    let rect = &svg[start..end];
    Some((
        extract_svg_attr(rect, "x")?.parse().ok()?,
        extract_svg_attr(rect, "y")?.parse().ok()?,
        extract_svg_attr(rect, "width")?.parse().ok()?,
        extract_svg_attr(rect, "height")?.parse().ok()?,
    ))
}

fn plot_mark_range(svg: &str) -> Option<(usize, usize)> {
    let start = svg.find(PLOT_CLIP_GROUP)? + PLOT_CLIP_GROUP.len();
    let end = svg[start..].find("</g>")? + start;
    Some((start, end))
}

fn extract_svg_attr<'a>(element: &'a str, attr: &str) -> Option<&'a str> {
    let marker = format!(r#"{attr}=""#);
    let start = element.find(&marker)? + marker.len();
    let end = element[start..].find('"')? + start;
    Some(&element[start..end])
}

fn is_left_tick_label_line(line: &str, plot_x: f64) -> bool {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("<text ") || trimmed.contains("transform=\"rotate(") {
        return false;
    }
    if !trimmed.contains("text-anchor=\"end\"")
        || !trimmed.contains("dominant-baseline=\"central\"")
    {
        return false;
    }
    let Some(x) = extract_svg_attr(trimmed, "x").and_then(|value| value.parse::<f64>().ok()) else {
        return false;
    };
    x < plot_x
}

fn is_left_tick_line(line: &str, plot_x: f64) -> bool {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("<line ") {
        return false;
    }
    let Some(x1) = extract_svg_attr(trimmed, "x1").and_then(|value| value.parse::<f64>().ok())
    else {
        return false;
    };
    let Some(x2) = extract_svg_attr(trimmed, "x2").and_then(|value| value.parse::<f64>().ok())
    else {
        return false;
    };
    (x1 - plot_x).abs() <= 0.25 && x2 < plot_x
}

fn offset_detail_mark_positions(
    svg: &str,
    table: &DataFrame,
    x_offset_px: f64,
    y_offset_px: f64,
) -> Result<String> {
    let media_types = table.column("media_type")?.str()?;
    let media_families = table
        .column("media_family")
        .ok()
        .and_then(|column| column.str().ok());
    let Some((range_start, range_end)) = plot_mark_range(svg) else {
        return Ok(svg.to_string());
    };

    let mut output = String::with_capacity(svg.len() + table.height() * 24);
    let mut cursor = range_start;
    let mut last_copied = 0usize;
    let mut point_index = 0usize;

    while let Some(relative_tag_start) = svg[cursor..range_end].find('<') {
        let tag_start = cursor + relative_tag_start;
        let Some(tag_end_relative) = svg[tag_start..range_end].find("/>") else {
            break;
        };
        let tag_end = tag_start + tag_end_relative + 2;
        let tag = &svg[tag_start..tag_end];
        let tag_name = tag
            .trim_start_matches('<')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches('/');

        if !matches!(tag_name, "circle" | "rect") {
            cursor = tag_end;
            continue;
        }

        output.push_str(&svg[last_copied..tag_start]);
        let media_type = media_types.get(point_index).unwrap_or("unknown");
        let media_family = media_families
            .as_ref()
            .and_then(|families| families.get(point_index))
            .unwrap_or_else(|| {
                if media_type.starts_with("video/") {
                    "video"
                } else {
                    "image"
                }
            });
        let (dx, dy) = if media_family == "video" {
            (x_offset_px, y_offset_px)
        } else {
            (-x_offset_px, -y_offset_px)
        };

        let shifted = match tag_name {
            "circle" => shift_svg_numeric_attrs(tag, &[("cx", dx), ("cy", dy)]),
            "rect" => shift_svg_numeric_attrs(tag, &[("x", dx), ("y", dy)]),
            _ => tag.to_string(),
        };
        output.push_str(&shifted);

        last_copied = tag_end;
        cursor = tag_end;
        point_index += 1;
    }

    output.push_str(&svg[last_copied..]);
    Ok(output)
}

fn shift_svg_numeric_attrs(tag: &str, attrs: &[(&str, f64)]) -> String {
    let mut shifted = tag.to_string();
    for (attr, delta) in attrs {
        shifted = shift_svg_numeric_attr(&shifted, attr, *delta);
    }
    shifted
}

fn shift_svg_numeric_attr(tag: &str, attr: &str, delta: f64) -> String {
    let marker = format!(r#"{attr}=""#);
    let Some(value_start) = tag.find(&marker).map(|index| index + marker.len()) else {
        return tag.to_string();
    };
    let Some(value_end_relative) = tag[value_start..].find('"') else {
        return tag.to_string();
    };
    let value_end = value_start + value_end_relative;
    let Ok(value) = tag[value_start..value_end].parse::<f64>() else {
        return tag.to_string();
    };

    let mut output = String::with_capacity(tag.len() + 8);
    output.push_str(&tag[..value_start]);
    output.push_str(&format!("{:.3}", value + delta));
    output.push_str(&tag[value_end..]);
    output
}

fn annotate_overview_svg(svg: &str, table: &DataFrame) -> Result<String> {
    let deployments = table.column("deployment")?.str()?;
    let bucket_labels = table.column("bucket_label")?.str()?;
    let event_counts = table.column("event_count")?.i64()?;
    let mut titles = Vec::with_capacity(table.height());

    for index in 0..table.height() {
        let deployment = deployments.get(index).unwrap_or("");
        let bucket_label = bucket_labels.get(index).unwrap_or("");
        let event_count = event_counts.get(index).unwrap_or_default().max(0) as usize;
        titles.push(format!(
            "Deployment: {deployment}\nBucket: {bucket_label}\nMedia count: {}",
            format_count(event_count)
        ));
    }

    Ok(inject_plot_titles(svg, &titles, &["rect"]))
}

fn annotate_detail_svg(svg: &str, table: &DataFrame) -> Result<String> {
    let timestamps = table.column("timestamp")?.cast(&DataType::Int64)?;
    let timestamp_values = timestamps.i64()?;
    let hours = table.column("hour_of_day")?.f64()?;
    let paths = table.column("path")?.str()?;
    let media_types = table.column("media_type")?.str()?;
    let media_families = table.column("media_family")?.str()?;
    let mut titles = Vec::with_capacity(table.height());

    for index in 0..table.height() {
        let timestamp_ns = timestamp_values.get(index).unwrap_or_default();
        let hour_of_day = hours.get(index).unwrap_or_default();
        let path = paths.get(index).unwrap_or("");
        let media_type = media_types.get(index).unwrap_or("unknown");
        let media_family = media_families.get(index).unwrap_or("image");
        titles.push(format!(
            "Timestamp: {}\nHour: {}\nMedia: {media_family}\nMedia type: {media_type}\nPath: {path}",
            format_timestamp_ns(timestamp_ns),
            format_hour_label(hour_of_day)
        ));
    }

    Ok(inject_plot_titles(svg, &titles, &["circle", "rect"]))
}

fn annotate_hour_heatmap_svg(svg: &str, table: &DataFrame) -> Result<String> {
    let deployments = table.column("deployment")?.str()?;
    let hour_labels = table.column("hour_label")?.str()?;
    let event_counts = table.column("event_count")?.i64()?;
    let mut titles = Vec::with_capacity(table.height());

    for index in 0..table.height() {
        let deployment = deployments.get(index).unwrap_or("");
        let hour_label = hour_labels.get(index).unwrap_or("");
        let event_count = event_counts.get(index).unwrap_or_default().max(0) as usize;
        titles.push(format!(
            "Deployment: {deployment}\nHour: {hour_label}\nMedia count: {}",
            format_count(event_count)
        ));
    }

    Ok(inject_plot_titles(svg, &titles, &["rect"]))
}

fn inject_plot_titles(svg: &str, titles: &[String], allowed_tags: &[&str]) -> String {
    let mut output = String::with_capacity(svg.len() + titles.len() * 72);
    let mut last_copied = 0usize;
    let mut title_index = 0usize;

    let Some((range_start, range_end)) = plot_mark_range(svg) else {
        return svg.to_string();
    };

    let mut cursor = range_start;
    while let Some(relative_tag_start) = svg[cursor..range_end].find('<') {
        let tag_start = cursor + relative_tag_start;
        let Some(tag_end_relative) = svg[tag_start..range_end].find("/>") else {
            break;
        };
        let tag_end = tag_start + tag_end_relative + 2;
        let tag_body = &svg[tag_start + 1..tag_end - 2];
        let tag_name = tag_body
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches('/');

        if !allowed_tags.contains(&tag_name) {
            cursor = tag_end;
            continue;
        }

        output.push_str(&svg[last_copied..tag_start]);

        if let Some(title) = titles.get(title_index) {
            output.push_str(&svg[tag_start..tag_end - 2]);
            output.push('>');
            output.push_str("<title>");
            output.push_str(&escape_html(title));
            output.push_str("</title></");
            output.push_str(tag_name);
            output.push('>');
            title_index += 1;
        } else {
            output.push_str(&svg[tag_start..tag_end]);
        }

        last_copied = tag_end;
        cursor = tag_end;
    }

    output.push_str(&svg[last_copied..]);
    output
}

fn format_timestamp_ns(timestamp_ns: i64) -> String {
    let seconds = timestamp_ns.div_euclid(1_000_000_000);
    let nanos = timestamp_ns.rem_euclid(1_000_000_000) as u32;

    DateTime::from_timestamp(seconds, nanos)
        .map(|value| value.naive_utc().format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| timestamp_ns.to_string())
}

fn format_hour_label(hour_of_day: f64) -> String {
    let total_minutes = (hour_of_day * 60.0).round() as i64;
    let hours = total_minutes.div_euclid(60);
    let minutes = total_minutes.rem_euclid(60);
    format!("{hours:02}:{minutes:02}")
}

fn format_minute_tick_label(minute_of_day: f64) -> String {
    let total_minutes = minute_of_day.round() as i64;
    let hours = total_minutes.div_euclid(60);
    let minutes = total_minutes.rem_euclid(60);
    format!("{hours:02}:{minutes:02}")
}

fn is_rotated_bottom_tick_label(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("<text ")
        && trimmed.contains("dominant-baseline=\"hanging\"")
        && trimmed.contains("transform=\"rotate(")
}

fn force_zero_event_cells(svg: &str, fill: &str) -> String {
    rewrite_plot_rects(svg, |rect| {
        if rect.contains("<title>") && extract_event_count(rect) == Some(0) {
            replace_rect_fill(rect, fill)
        } else {
            rect.to_string()
        }
    })
}

fn boost_low_positive_event_cells(svg: &str, threshold: usize, fill: &str) -> String {
    rewrite_plot_rects(svg, |rect| match extract_event_count(rect) {
        Some(event_count) if (1..=threshold).contains(&event_count) => {
            replace_rect_fill(rect, fill)
        }
        _ => rect.to_string(),
    })
}

fn rewrite_plot_rects(svg: &str, mut rewrite: impl FnMut(&str) -> String) -> String {
    let Some((range_start, range_end)) = plot_mark_range(svg) else {
        return svg.to_string();
    };

    let mut output = String::with_capacity(svg.len());
    let mut cursor = range_start;
    let mut last_copied = 0usize;

    while let Some(relative_rect_start) = svg[cursor..range_end].find("<rect") {
        let rect_start = cursor + relative_rect_start;
        output.push_str(&svg[last_copied..rect_start]);

        let rect_tail = &svg[rect_start..range_end];
        let self_closing_end = rect_tail.find("/>").map(|index| rect_start + index + 2);
        let paired_end = rect_tail
            .find("</rect>")
            .map(|index| rect_start + index + "</rect>".len());
        let Some(rect_end) = self_closing_end.into_iter().chain(paired_end).min() else {
            break;
        };
        let rect = &svg[rect_start..rect_end];

        output.push_str(&rewrite(rect));

        last_copied = rect_end;
        cursor = rect_end;
    }

    output.push_str(&svg[last_copied..]);
    output
}

fn replace_rect_fill(rect: &str, fill: &str) -> String {
    let Some(fill_attr_start) = rect.find("fill=\"") else {
        return rect.to_string();
    };
    let fill_value_start = fill_attr_start + "fill=\"".len();
    let Some(fill_value_end_relative) = rect[fill_value_start..].find('"') else {
        return rect.to_string();
    };
    let fill_value_end = fill_value_start + fill_value_end_relative;

    let mut output = String::with_capacity(rect.len() + fill.len());
    output.push_str(&rect[..fill_value_start]);
    output.push_str(fill);
    output.push_str(&rect[fill_value_end..]);
    output
}

fn extract_event_count(rect: &str) -> Option<usize> {
    let marker = "Media count:";
    let value_start = rect.find(marker)? + marker.len();
    let value_end_relative = rect[value_start..]
        .find('<')
        .or_else(|| rect[value_start..].find('\n'))
        .unwrap_or(rect.len() - value_start);
    let raw = rect[value_start..value_start + value_end_relative]
        .trim()
        .replace(',', "");
    raw.parse().ok()
}

fn force_zero_legend_stop(svg: &str, fill: &str) -> String {
    let gradient_marker = "<linearGradient id=\"grad_event_count\"";
    let zero_stop_marker = "offset=\"100.0%\" stop-color=\"";
    let Some(gradient_start) = svg.find(gradient_marker) else {
        return svg.to_string();
    };
    let Some(gradient_end_relative) = svg[gradient_start..].find("</linearGradient>") else {
        return svg.to_string();
    };
    let gradient_end = gradient_start + gradient_end_relative;
    let gradient = &svg[gradient_start..gradient_end];
    let Some(stop_start_relative) = gradient.find(zero_stop_marker) else {
        return svg.to_string();
    };
    let stop_start = gradient_start + stop_start_relative + zero_stop_marker.len();
    let Some(stop_end_relative) = svg[stop_start..].find('"') else {
        return svg.to_string();
    };
    let stop_end = stop_start + stop_end_relative;

    let mut output = String::with_capacity(svg.len());
    output.push_str(&svg[..stop_start]);
    output.push_str(fill);
    output.push_str(&svg[stop_end..]);
    output
}

fn relabel_legend_titles(svg: &str, replacements: &[(&str, &str)]) -> String {
    let mut output = svg.to_string();
    for (from, to) in replacements {
        output = output.replace(&format!(">{from}</text>"), &format!(">{to}</text>"));
    }
    output
}

fn display_font() -> &'static str {
    "'Iowan Old Style', 'Palatino Linotype', 'Book Antiqua', Georgia, serif"
}

fn sans_font() -> &'static str {
    "'IBM Plex Sans', 'Avenir Next', 'Segoe UI Variable', 'Segoe UI', 'Noto Sans', sans-serif"
}
