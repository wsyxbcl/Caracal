use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
#[cfg(not(target_arch = "wasm32"))]
use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
#[cfg(not(target_arch = "wasm32"))]
use clap::ValueEnum;
use polars::prelude::*;
use polars_io::mmap::MmapBytesReader;
use polars_io::prelude::{CsvReadOptions, SerReader};

const CSV_INFER_SCHEMA_LENGTH: usize = 1_000;

#[cfg_attr(not(target_arch = "wasm32"), derive(ValueEnum))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum OverviewBucket {
    Day,
    Week,
    Month,
}

impl OverviewBucket {
    pub const ALL: [OverviewBucket; 3] = [
        OverviewBucket::Day,
        OverviewBucket::Week,
        OverviewBucket::Month,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            OverviewBucket::Day => "day",
            OverviewBucket::Week => "week",
            OverviewBucket::Month => "month",
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn axis_label(self) -> &'static str {
        match self {
            OverviewBucket::Day => "Day",
            OverviewBucket::Week => "Week start",
            OverviewBucket::Month => "Month start",
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn display_name(self) -> &'static str {
        match self {
            OverviewBucket::Day => "daily",
            OverviewBucket::Week => "weekly",
            OverviewBucket::Month => "monthly",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeploymentSummary {
    pub deployment: String,
    pub order: usize,
    pub event_count: usize,
    pub first_seen: NaiveDateTime,
    pub last_seen: NaiveDateTime,
    pub media_counts: BTreeMap<String, usize>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub has_gps: bool,
}

impl DeploymentSummary {
    pub fn media_breakdown(&self) -> String {
        self.media_counts
            .iter()
            .map(|(kind, count)| format!("{kind} {count}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Clone)]
pub struct PreparedData {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub csv_path: PathBuf,
    pub min_timestamp: NaiveDateTime,
    pub max_timestamp: NaiveDateTime,
    pub deployments: Vec<DeploymentSummary>,
    pub events: DataFrame,
    pub detail_tables: BTreeMap<String, DataFrame>,
    pub overview_tables: BTreeMap<OverviewBucket, DataFrame>,
    pub hour_heatmap: DataFrame,
}

impl PreparedData {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(csv_path: &Path) -> Result<Self> {
        let file = File::open(csv_path)
            .with_context(|| format!("failed to open CSV at {}", csv_path.display()))?;
        Self::from_reader(file, csv_path.to_path_buf())
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn from_csv_text(csv_content: &str) -> Result<Self> {
        Self::from_reader(
            Cursor::new(csv_content.as_bytes()),
            PathBuf::from("uploaded.csv"),
        )
    }

    fn from_reader<R>(mut reader: R, csv_path: PathBuf) -> Result<Self>
    where
        R: Read + Seek + Send + Sync + MmapBytesReader,
    {
        let headers = inspect_csv_headers(&mut reader)?;
        let frame = CsvReadOptions::default()
            .with_has_header(true)
            .with_infer_schema_length(Some(CSV_INFER_SCHEMA_LENGTH))
            .with_schema_overwrite(headers.schema_overwrite())
            .into_reader_with_file_handle(reader)
            .finish()
            .context("failed to read CSV with Polars")?;
        let frame = normalize_frame_headers(frame, &headers)?;

        let indices = ColumnIndices::from_dataframe(&frame)?;
        let mut rows = Vec::with_capacity(frame.height());

        for row_index in 0..frame.height() {
            rows.push(EventRow::from_dataframe_row(&frame, &indices, row_index)?);
        }

        if rows.is_empty() {
            bail!("the input CSV does not contain any rows");
        }

        let min_timestamp = rows
            .iter()
            .map(|row| row.timestamp)
            .min()
            .expect("rows checked non-empty");
        let max_timestamp = rows
            .iter()
            .map(|row| row.timestamp)
            .max()
            .expect("rows checked non-empty");

        let deployments = summarize_rows(&rows);

        let order_map = deployments
            .iter()
            .map(|deployment| (deployment.deployment.clone(), deployment.order))
            .collect::<HashMap<_, _>>();

        rows.sort_by(|left, right| {
            order_map[&left.deployment]
                .cmp(&order_map[&right.deployment])
                .then_with(|| left.timestamp.cmp(&right.timestamp))
                .then_with(|| left.media_type.cmp(&right.media_type))
        });

        let events = build_events_table(&rows, &order_map)?;
        let detail_tables = build_detail_tables(&rows, &deployments)?;
        let hour_heatmap = build_hour_heatmap(&rows, &deployments, &order_map)?;

        let mut overview_tables = BTreeMap::new();
        for bucket in OverviewBucket::ALL {
            overview_tables.insert(
                bucket,
                build_overview_table(
                    bucket,
                    &rows,
                    &deployments,
                    &order_map,
                    min_timestamp,
                    max_timestamp,
                )?,
            );
        }

        Ok(Self {
            csv_path,
            min_timestamp,
            max_timestamp,
            deployments,
            events,
            detail_tables,
            overview_tables,
            hour_heatmap,
        })
    }

    pub fn default_deployment(&self) -> &str {
        &self.deployments[0].deployment
    }

    pub fn deployment_summary(&self, deployment: &str) -> Result<&DeploymentSummary> {
        self.deployments
            .iter()
            .find(|item| item.deployment == deployment)
            .ok_or_else(|| anyhow!("unknown deployment: {deployment}"))
    }

    pub fn detail_table(&self, deployment: &str) -> Result<&DataFrame> {
        self.detail_tables
            .get(deployment)
            .ok_or_else(|| anyhow!("unknown deployment: {deployment}"))
    }

    pub fn overview_table(&self, bucket: OverviewBucket) -> Result<&DataFrame> {
        self.overview_tables
            .get(&bucket)
            .ok_or_else(|| anyhow!("missing overview table for {}", bucket.slug()))
    }
}

#[derive(Clone, Debug)]
struct EventRow {
    deployment: String,
    timestamp: NaiveDateTime,
    path: String,
    media_type: String,
    media_family: String,
    has_gps: bool,
}

impl EventRow {
    fn from_dataframe_row(
        frame: &DataFrame,
        indices: &ColumnIndices,
        row_index: usize,
    ) -> Result<Self> {
        let row_number = row_index + 2;

        let deployment = required_trimmed_string(
            frame,
            indices.deployment,
            row_index,
            "deployment",
            row_number,
        )?;
        let timestamp = parse_effective_timestamp(frame, indices, row_index, row_number)?;
        let path = optional_trimmed_string(frame, indices.path, row_index)?.unwrap_or_default();
        let media_type = indices
            .media_type
            .map(|index| optional_trimmed_string(frame, index, row_index))
            .transpose()?
            .flatten()
            .as_deref()
            .map(normalize_media_type)
            .unwrap_or_else(|| infer_media_type_from_path(&path));
        let media_family = media_family(&media_type, &path).to_string();

        let latitude = read_gps_value(frame, indices.latitude, row_index)?;
        let longitude = read_gps_value(frame, indices.longitude, row_index)?;
        let has_gps = latitude.is_some_and(|value| value != 0.0)
            && longitude.is_some_and(|value| value != 0.0);

        Ok(Self {
            deployment,
            timestamp,
            path,
            media_type,
            media_family,
            has_gps,
        })
    }
}

/// Reads an optional latitude/longitude cell as a coordinate. Empty or
/// unparseable cells are treated as absent (`None`), which callers read as
/// "no GPS" rather than as a zero coordinate.
fn read_gps_value(
    frame: &DataFrame,
    column_index: Option<usize>,
    row_index: usize,
) -> Result<Option<f64>> {
    let Some(column_index) = column_index else {
        return Ok(None);
    };
    Ok(optional_trimmed_string(frame, column_index, row_index)?
        .and_then(|value| value.parse::<f64>().ok()))
}

fn summarize_rows(rows: &[EventRow]) -> Vec<DeploymentSummary> {
    let mut stats = HashMap::<String, DeploymentAccumulator>::new();
    for row in rows {
        let entry = stats
            .entry(row.deployment.clone())
            .or_insert_with(|| DeploymentAccumulator::new(row.timestamp));
        entry.record(row.timestamp, &row.media_family, row.has_gps);
    }

    let mut deployments = stats
        .into_iter()
        .map(|(deployment, stat)| DeploymentSummary {
            deployment,
            order: 0,
            event_count: stat.event_count,
            first_seen: stat.first_seen,
            last_seen: stat.last_seen,
            media_counts: stat.media_counts,
            has_gps: stat.has_gps,
        })
        .collect::<Vec<_>>();

    deployments
        .sort_by(|left, right| compare_deployment_names(&left.deployment, &right.deployment));

    for (order, deployment) in deployments.iter_mut().enumerate() {
        deployment.order = order;
    }

    deployments
}

#[derive(Clone, Copy, Debug)]
struct ColumnIndices {
    path: usize,
    deployment: usize,
    datetime: usize,
    xmp_update_datetime: Option<usize>,
    media_type: Option<usize>,
    latitude: Option<usize>,
    longitude: Option<usize>,
}

impl ColumnIndices {
    fn from_dataframe(frame: &DataFrame) -> Result<Self> {
        let path = frame
            .get_column_index("path")
            .context("missing required column 'path'")?;
        let deployment = frame
            .get_column_index("deployment")
            .context("missing required column 'deployment'")?;
        let datetime = frame
            .get_column_index("datetime")
            .context("missing required column 'datetime'")?;
        let xmp_update_datetime = frame.get_column_index("xmp_update_datetime");
        let media_type = frame.get_column_index("media_type");
        let latitude = frame.get_column_index("latitude");
        let longitude = frame.get_column_index("longitude");

        Ok(Self {
            path,
            deployment,
            datetime,
            xmp_update_datetime,
            media_type,
            latitude,
            longitude,
        })
    }
}

#[derive(Clone, Debug)]
struct CsvHeaders {
    raw_headers: Vec<String>,
    normalized_headers: Vec<String>,
}

impl CsvHeaders {
    fn schema_overwrite(&self) -> Option<SchemaRef> {
        let fields = self
            .raw_headers
            .iter()
            .zip(self.normalized_headers.iter())
            .filter_map(|(raw, normalized)| match normalized.as_str() {
                "path" | "deployment" | "datetime" | "media_type" | "xmp_update_datetime"
                | "latitude" | "longitude" => {
                    Some(Field::new(raw.as_str().into(), DataType::String))
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        (!fields.is_empty()).then(|| Arc::new(Schema::from_iter(fields)))
    }
}

fn inspect_csv_headers<R>(reader: &mut R) -> Result<CsvHeaders>
where
    R: Read + Seek,
{
    let mut header_line = String::new();
    {
        let mut buffered = BufReader::new(&mut *reader);
        buffered
            .read_line(&mut header_line)
            .context("failed to read CSV headers")?;
    }

    if header_line.is_empty() {
        bail!("the input CSV is empty");
    }

    reader
        .seek(SeekFrom::Start(0))
        .context("failed to rewind CSV reader after header scan")?;

    let raw_headers = parse_csv_header_line(&header_line);
    let normalized_headers = raw_headers
        .iter()
        .map(|header| normalize_header_name(header))
        .collect::<Vec<_>>();

    Ok(CsvHeaders {
        raw_headers,
        normalized_headers,
    })
}

fn parse_csv_header_line(line: &str) -> Vec<String> {
    line.trim_end_matches(['\r', '\n'])
        .split(',')
        .map(|header| header.trim().trim_matches('"').to_string())
        .collect()
}

fn normalize_header_name(header: &str) -> String {
    header.trim().trim_start_matches('\u{feff}').to_string()
}

fn normalize_frame_headers(mut frame: DataFrame, headers: &CsvHeaders) -> Result<DataFrame> {
    for (raw, normalized) in headers
        .raw_headers
        .iter()
        .zip(headers.normalized_headers.iter())
    {
        if raw != normalized
            && frame.get_column_index(raw).is_some()
            && frame.get_column_index(normalized).is_none()
        {
            frame
                .rename(raw, normalized.as_str().into())
                .with_context(|| format!("failed to normalize CSV header '{raw}'"))?;
        }
    }

    Ok(frame)
}

fn required_trimmed_string(
    frame: &DataFrame,
    column_index: usize,
    row_index: usize,
    column_name: &str,
    row_number: usize,
) -> Result<String> {
    optional_trimmed_string(frame, column_index, row_index)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("missing {column_name} at row {row_number}"))
}

fn optional_trimmed_string(
    frame: &DataFrame,
    column_index: usize,
    row_index: usize,
) -> Result<Option<String>> {
    let value = frame
        .get_columns()
        .get(column_index)
        .ok_or_else(|| anyhow!("column index {column_index} is out of bounds"))?
        .get(row_index)
        .with_context(|| format!("failed to read row {}", row_index + 2))?;

    match value {
        AnyValue::Null => Ok(None),
        other => Ok(Some(other.str_value().trim().to_string())),
    }
}

fn parse_effective_timestamp(
    frame: &DataFrame,
    indices: &ColumnIndices,
    row_index: usize,
    row_number: usize,
) -> Result<NaiveDateTime> {
    if let Some(update_index) = indices.xmp_update_datetime {
        if let Some(raw_update) = optional_trimmed_string(frame, update_index, row_index)?
            .filter(|value| !value.is_empty())
        {
            return parse_datetime_text(&raw_update, row_number, "xmp_update_datetime");
        }
    }

    let raw_datetime =
        required_trimmed_string(frame, indices.datetime, row_index, "datetime", row_number)?;
    parse_datetime_text(&raw_datetime, row_number, "datetime")
}

fn parse_datetime_text(raw: &str, row_number: usize, column_name: &str) -> Result<NaiveDateTime> {
    let value = raw.trim();
    if value.is_empty() {
        bail!("missing {column_name} at row {row_number}");
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed.naive_utc());
    }

    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y/%m/%d %H:%M:%S%.f",
        "%Y/%m/%dT%H:%M:%S%.f",
        "%Y:%m:%d %H:%M:%S%.f",
    ] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(value, format) {
            return Ok(parsed);
        }
    }

    for format in [
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y/%m/%d %H:%M:%S%.f%:z",
        "%Y/%m/%dT%H:%M:%S%.f%:z",
    ] {
        if let Ok(parsed) = DateTime::parse_from_str(value, format) {
            return Ok(parsed.naive_utc());
        }
    }

    for format in ["%Y-%m-%d", "%Y/%m/%d", "%Y:%m:%d"] {
        if let Ok(parsed) = NaiveDate::parse_from_str(value, format) {
            return parsed
                .and_hms_opt(0, 0, 0)
                .ok_or_else(|| anyhow!("datetime is out of range at row {row_number}"));
        }
    }

    bail!("failed to parse {column_name} value '{value}' at row {row_number}")
}

fn compare_deployment_names(left: &str, right: &str) -> Ordering {
    let mut left_index = 0;
    let mut right_index = 0;
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();

    while left_index < left.len() && right_index < right.len() {
        let left_digit = left_bytes[left_index].is_ascii_digit();
        let right_digit = right_bytes[right_index].is_ascii_digit();

        if left_digit && right_digit {
            let left_end = advance_ascii_digits(left_bytes, left_index);
            let right_end = advance_ascii_digits(right_bytes, right_index);
            let ordering =
                compare_numeric_chunks(&left[left_index..left_end], &right[right_index..right_end]);
            if ordering != Ordering::Equal {
                return ordering;
            }
            left_index = left_end;
            right_index = right_end;
            continue;
        }

        let left_char = left[left_index..]
            .chars()
            .next()
            .expect("left_index checked against len");
        let right_char = right[right_index..]
            .chars()
            .next()
            .expect("right_index checked against len");

        let ordering = left_char
            .to_ascii_lowercase()
            .cmp(&right_char.to_ascii_lowercase());
        if ordering != Ordering::Equal {
            return ordering;
        }

        left_index += left_char.len_utf8();
        right_index += right_char.len_utf8();
    }

    left.len().cmp(&right.len())
}

fn advance_ascii_digits(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    end
}

fn compare_numeric_chunks(left: &str, right: &str) -> Ordering {
    let left_trimmed = left.trim_start_matches('0');
    let right_trimmed = right.trim_start_matches('0');
    let left_normalized = if left_trimmed.is_empty() {
        "0"
    } else {
        left_trimmed
    };
    let right_normalized = if right_trimmed.is_empty() {
        "0"
    } else {
        right_trimmed
    };

    left_normalized
        .len()
        .cmp(&right_normalized.len())
        .then_with(|| left_normalized.cmp(right_normalized))
        .then_with(|| left.len().cmp(&right.len()))
}

#[derive(Clone, Debug)]
struct DeploymentAccumulator {
    event_count: usize,
    first_seen: NaiveDateTime,
    last_seen: NaiveDateTime,
    media_counts: BTreeMap<String, usize>,
    has_gps: bool,
}

impl DeploymentAccumulator {
    fn new(timestamp: NaiveDateTime) -> Self {
        Self {
            event_count: 0,
            first_seen: timestamp,
            last_seen: timestamp,
            media_counts: BTreeMap::new(),
            has_gps: false,
        }
    }

    fn record(&mut self, timestamp: NaiveDateTime, media_type: &str, has_gps: bool) {
        self.event_count += 1;
        self.first_seen = self.first_seen.min(timestamp);
        self.last_seen = self.last_seen.max(timestamp);
        *self.media_counts.entry(media_type.to_string()).or_insert(0) += 1;
        self.has_gps |= has_gps;
    }
}

fn build_events_table(rows: &[EventRow], order_map: &HashMap<String, usize>) -> Result<DataFrame> {
    let mut deployments = Vec::with_capacity(rows.len());
    let mut deployment_order = Vec::with_capacity(rows.len());
    let mut timestamps = Vec::with_capacity(rows.len());
    let mut day_labels = Vec::with_capacity(rows.len());
    let mut hours = Vec::with_capacity(rows.len());
    let mut minutes = Vec::with_capacity(rows.len());
    let mut event_index = Vec::with_capacity(rows.len());
    let mut paths = Vec::with_capacity(rows.len());
    let mut media_types = Vec::with_capacity(rows.len());
    let mut media_families = Vec::with_capacity(rows.len());

    let mut current_deployment = None::<&str>;
    let mut index_within = 0_i32;

    for row in rows {
        if current_deployment != Some(row.deployment.as_str()) {
            current_deployment = Some(row.deployment.as_str());
            index_within = 0;
        }

        deployments.push(row.deployment.clone());
        deployment_order.push(order_map[&row.deployment] as i32);
        timestamps.push(timestamp_to_ns(row.timestamp)?);
        day_labels.push(day_label(row.timestamp));
        hours.push(hour_of_day(row.timestamp));
        minutes.push(minute_of_day(row.timestamp));
        event_index.push(index_within);
        paths.push(row.path.clone());
        media_types.push(row.media_type.clone());
        media_families.push(row.media_family.clone());
        index_within += 1;
    }

    build_event_dataframe(
        deployments,
        deployment_order,
        timestamps,
        day_labels,
        hours,
        minutes,
        event_index,
        paths,
        media_types,
        media_families,
    )
}

fn build_detail_tables(
    rows: &[EventRow],
    deployments: &[DeploymentSummary],
) -> Result<BTreeMap<String, DataFrame>> {
    let mut grouped = BTreeMap::<String, Vec<&EventRow>>::new();
    for row in rows {
        grouped.entry(row.deployment.clone()).or_default().push(row);
    }

    let mut tables = BTreeMap::new();
    for deployment in deployments {
        let group = grouped.get(&deployment.deployment).ok_or_else(|| {
            anyhow!(
                "missing event rows for deployment {}",
                deployment.deployment
            )
        })?;

        let mut deployments_col = Vec::with_capacity(group.len());
        let mut deployment_order = Vec::with_capacity(group.len());
        let mut timestamps = Vec::with_capacity(group.len());
        let mut day_labels = Vec::with_capacity(group.len());
        let mut hours = Vec::with_capacity(group.len());
        let mut minutes = Vec::with_capacity(group.len());
        let mut event_index = Vec::with_capacity(group.len());
        let mut paths = Vec::with_capacity(group.len());
        let mut media_types = Vec::with_capacity(group.len());
        let mut media_families = Vec::with_capacity(group.len());

        for (index, row) in group.iter().enumerate() {
            deployments_col.push(row.deployment.clone());
            deployment_order.push(deployment.order as i32);
            timestamps.push(timestamp_to_ns(row.timestamp)?);
            day_labels.push(day_label(row.timestamp));
            hours.push(hour_of_day(row.timestamp));
            minutes.push(minute_of_day(row.timestamp));
            event_index.push(index as i32);
            paths.push(row.path.clone());
            media_types.push(row.media_type.clone());
            media_families.push(row.media_family.clone());
        }

        tables.insert(
            deployment.deployment.clone(),
            build_event_dataframe(
                deployments_col,
                deployment_order,
                timestamps,
                day_labels,
                hours,
                minutes,
                event_index,
                paths,
                media_types,
                media_families,
            )?,
        );
    }

    Ok(tables)
}

fn build_event_dataframe(
    deployments: Vec<String>,
    deployment_order: Vec<i32>,
    timestamps: Vec<i64>,
    day_labels: Vec<String>,
    hours: Vec<f64>,
    minutes: Vec<f64>,
    event_index: Vec<i32>,
    paths: Vec<String>,
    media_types: Vec<String>,
    media_families: Vec<String>,
) -> Result<DataFrame> {
    DataFrame::new(vec![
        Series::new("deployment".into(), deployments).into(),
        Series::new("deployment_order".into(), deployment_order).into(),
        Series::new("timestamp".into(), timestamps).into(),
        Series::new("day_label".into(), day_labels).into(),
        Series::new("hour_of_day".into(), hours).into(),
        Series::new("minute_of_day".into(), minutes).into(),
        Series::new("event_index".into(), event_index).into(),
        Series::new("path".into(), paths).into(),
        Series::new("media_type".into(), media_types).into(),
        Series::new("media_family".into(), media_families).into(),
    ])
    .context("failed to build events dataframe")
}

fn build_overview_table(
    bucket: OverviewBucket,
    rows: &[EventRow],
    deployments: &[DeploymentSummary],
    order_map: &HashMap<String, usize>,
    min_timestamp: NaiveDateTime,
    max_timestamp: NaiveDateTime,
) -> Result<DataFrame> {
    let bucket_starts = enumerate_buckets(bucket, min_timestamp, max_timestamp);
    let mut counts = HashMap::<(usize, i64), i64>::new();

    for row in rows {
        let deployment_order = order_map[&row.deployment];
        let bucket_start = bucket_floor(bucket, row.timestamp);
        let bucket_ns = timestamp_to_ns(bucket_start)?;
        *counts.entry((deployment_order, bucket_ns)).or_insert(0) += 1;
    }

    let total_cells = deployments.len() * bucket_starts.len();
    let mut deployment_col = Vec::with_capacity(total_cells);
    let mut order_col = Vec::with_capacity(total_cells);
    let mut bucket_ns_col = Vec::with_capacity(total_cells);
    let mut bucket_label_col = Vec::with_capacity(total_cells);
    let mut count_col = Vec::with_capacity(total_cells);

    for deployment in deployments.iter().rev() {
        for bucket_start in &bucket_starts {
            let bucket_ns = timestamp_to_ns(*bucket_start)?;
            deployment_col.push(deployment.deployment.clone());
            order_col.push(deployment.order as i32);
            bucket_ns_col.push(bucket_ns);
            bucket_label_col.push(format_bucket_label(bucket, *bucket_start));
            count_col.push(*counts.get(&(deployment.order, bucket_ns)).unwrap_or(&0));
        }
    }

    DataFrame::new(vec![
        Series::new("deployment".into(), deployment_col).into(),
        Series::new("deployment_order".into(), order_col).into(),
        Series::new("bucket_start".into(), bucket_ns_col).into(),
        Series::new("bucket_label".into(), bucket_label_col).into(),
        Series::new("event_count".into(), count_col).into(),
    ])
    .context("failed to build overview dataframe")
}

fn build_hour_heatmap(
    rows: &[EventRow],
    deployments: &[DeploymentSummary],
    order_map: &HashMap<String, usize>,
) -> Result<DataFrame> {
    let mut counts = HashMap::<(usize, u32), i64>::new();
    for row in rows {
        let deployment_order = order_map[&row.deployment];
        *counts
            .entry((deployment_order, row.timestamp.hour()))
            .or_insert(0) += 1;
    }

    let total_cells = deployments.len() * 24;
    let mut deployment_col = Vec::with_capacity(total_cells);
    let mut order_col = Vec::with_capacity(total_cells);
    let mut hour_value_col = Vec::with_capacity(total_cells);
    let mut hour_label_col = Vec::with_capacity(total_cells);
    let mut count_col = Vec::with_capacity(total_cells);

    for deployment in deployments.iter().rev() {
        for hour in 0..24_u32 {
            deployment_col.push(deployment.deployment.clone());
            order_col.push(deployment.order as i32);
            hour_value_col.push(hour as i32);
            hour_label_col.push(format!("{hour:02}:00"));
            count_col.push(*counts.get(&(deployment.order, hour)).unwrap_or(&0));
        }
    }

    DataFrame::new(vec![
        Series::new("deployment".into(), deployment_col).into(),
        Series::new("deployment_order".into(), order_col).into(),
        Series::new("hour".into(), hour_value_col).into(),
        Series::new("hour_label".into(), hour_label_col).into(),
        Series::new("event_count".into(), count_col).into(),
    ])
    .context("failed to build hour heatmap dataframe")
}

fn infer_media_type_from_path(path: &str) -> String {
    let trimmed = path.strip_suffix(".xmp").unwrap_or(path);
    let extension = trimmed
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or("unknown");

    match extension.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "png" => "image/png".to_string(),
        "mp4" => "video/mp4".to_string(),
        "mov" => "video/quicktime".to_string(),
        other => other.to_string(),
    }
}

fn normalize_media_type(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn media_family(media_type: &str, path: &str) -> &'static str {
    if media_type.starts_with("image/") {
        "image"
    } else if media_type.starts_with("video/") {
        "video"
    } else {
        match infer_media_type_from_path(path).as_str() {
            value if value.starts_with("video/") => "video",
            _ => "image",
        }
    }
}

fn bucket_floor(bucket: OverviewBucket, timestamp: NaiveDateTime) -> NaiveDateTime {
    let date = timestamp.date();

    match bucket {
        OverviewBucket::Day => date.and_hms_opt(0, 0, 0).expect("valid midnight"),
        OverviewBucket::Week => {
            let days_from_monday = date.weekday().num_days_from_monday() as i64;
            (date - Duration::days(days_from_monday))
                .and_hms_opt(0, 0, 0)
                .expect("valid monday midnight")
        }
        OverviewBucket::Month => NaiveDate::from_ymd_opt(date.year(), date.month(), 1)
            .expect("valid first day of month")
            .and_hms_opt(0, 0, 0)
            .expect("valid month midnight"),
    }
}

fn enumerate_buckets(
    bucket: OverviewBucket,
    min_timestamp: NaiveDateTime,
    max_timestamp: NaiveDateTime,
) -> Vec<NaiveDateTime> {
    let mut current = bucket_floor(bucket, min_timestamp);
    let end = bucket_floor(bucket, max_timestamp);
    let mut buckets = Vec::new();

    while current <= end {
        buckets.push(current);
        current = next_bucket(bucket, current);
    }

    buckets
}

fn next_bucket(bucket: OverviewBucket, current: NaiveDateTime) -> NaiveDateTime {
    match bucket {
        OverviewBucket::Day => current + Duration::days(1),
        OverviewBucket::Week => current + Duration::weeks(1),
        OverviewBucket::Month => {
            let year = current.date().year();
            let month = current.date().month();
            let (next_year, next_month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };

            NaiveDate::from_ymd_opt(next_year, next_month, 1)
                .expect("valid first day for next month")
                .and_hms_opt(0, 0, 0)
                .expect("valid next month midnight")
        }
    }
}

fn format_bucket_label(bucket: OverviewBucket, value: NaiveDateTime) -> String {
    match bucket {
        OverviewBucket::Day | OverviewBucket::Week => value.format("%Y-%m-%d").to_string(),
        OverviewBucket::Month => value.format("%Y-%m").to_string(),
    }
}

fn timestamp_to_ns(timestamp: NaiveDateTime) -> Result<i64> {
    timestamp
        .and_utc()
        .timestamp_nanos_opt()
        .ok_or_else(|| anyhow!("timestamp is out of nanosecond range: {timestamp}"))
}

fn hour_of_day(timestamp: NaiveDateTime) -> f64 {
    timestamp.hour() as f64 + timestamp.minute() as f64 / 60.0 + timestamp.second() as f64 / 3600.0
}

fn minute_of_day(timestamp: NaiveDateTime) -> f64 {
    timestamp.hour() as f64 * 60.0 + timestamp.minute() as f64 + timestamp.second() as f64 / 60.0
}

fn day_label(timestamp: NaiveDateTime) -> String {
    timestamp.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary<'a>(data: &'a PreparedData, name: &str) -> &'a DeploymentSummary {
        data.deployments
            .iter()
            .find(|deployment| deployment.deployment == name)
            .expect("deployment present")
    }

    #[test]
    fn gps_requires_both_coordinates_nonzero() {
        let csv = "path,deployment,datetime,latitude,longitude\n\
            a.jpg,withgps,2025-03-01 06:00:00,31.2,121.4\n\
            b.jpg,zeros,2025-03-01 06:00:00,0,0\n\
            c.jpg,partial,2025-03-01 06:00:00,31.2,0\n\
            d.jpg,empty,2025-03-01 06:00:00,,\n";
        let data = PreparedData::from_csv_text(csv).expect("parse csv");
        assert!(summary(&data, "withgps").has_gps);
        assert!(!summary(&data, "zeros").has_gps);
        assert!(!summary(&data, "partial").has_gps);
        assert!(!summary(&data, "empty").has_gps);
    }

    #[test]
    fn gps_marked_when_any_row_has_coordinates() {
        let csv = "path,deployment,datetime,latitude,longitude\n\
            a.jpg,mix,2025-03-01 06:00:00,,\n\
            b.jpg,mix,2025-03-02 06:00:00,31.2,121.4\n";
        let data = PreparedData::from_csv_text(csv).expect("parse csv");
        assert!(summary(&data, "mix").has_gps);
    }

    #[test]
    fn no_gps_columns_means_no_gps() {
        let csv = "path,deployment,datetime\n\
            a.jpg,nogps,2025-03-01 06:00:00\n";
        let data = PreparedData::from_csv_text(csv).expect("parse csv");
        assert!(!summary(&data, "nogps").has_gps);
    }

    #[test]
    fn single_coordinate_column_is_optional_and_not_gps() {
        // Only one of the two coordinate columns present: still parses, and a
        // single coordinate never counts as GPS.
        let csv = "path,deployment,datetime,latitude\n\
            a.jpg,latonly,2025-03-01 06:00:00,31.2\n";
        let data = PreparedData::from_csv_text(csv).expect("parse csv");
        assert!(!summary(&data, "latonly").has_gps);
    }
}
