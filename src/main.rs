mod data;
mod render;
mod report;
mod species;
mod util;
mod web_app;

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::data::{OverviewBucket, PreparedData};
use crate::util::{format_count, slugify};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Run the local WASM deployment timestamp visualizer or export static visuals"
)]
struct Cli {
    #[arg(long, global = true)]
    csv: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    ExportStatic(ExportStaticArgs),
    ExportReport(ExportReportArgs),
    ServeWasm(ServeWasmArgs),
}

#[derive(Args, Debug)]
struct ExportStaticArgs {
    #[arg(long, default_value = "output/static")]
    out_dir: PathBuf,

    #[arg(long)]
    deployment: Option<String>,

    #[arg(long, value_enum, default_value_t = OverviewBucket::Week)]
    overview_bucket: OverviewBucket,

    #[arg(long, value_enum, default_value_t = OutputFormat::Svg)]
    format: OutputFormat,

    #[arg(long, default_value_t = false)]
    all_details: bool,
}

#[derive(Args, Debug)]
struct ExportReportArgs {
    #[arg(long, default_value = "output/report/timestamp_report.html")]
    out: PathBuf,

    #[arg(long)]
    deployment: Option<String>,

    #[arg(long, value_enum, default_value_t = OverviewBucket::Week)]
    overview_bucket: OverviewBucket,

    #[arg(long, default_value_t = 6)]
    top_details: usize,
}

#[derive(Args, Debug)]
struct ServeWasmArgs {
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: SocketAddr,

    #[arg(long, default_value_t = false)]
    open: bool,
}

impl Default for ServeWasmArgs {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8787".parse().expect("default bind must parse"),
            open: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Svg,
    Png,
}

impl OutputFormat {
    fn extension(self) -> &'static str {
        match self {
            OutputFormat::Svg => "svg",
            OutputFormat::Png => "png",
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::ServeWasm(ServeWasmArgs::default())) {
        Command::ExportStatic(args) => {
            let data = PreparedData::load(require_csv(&cli.csv)?)?;
            export_static(&data, args)
        }
        Command::ExportReport(args) => {
            let data = PreparedData::load(require_csv(&cli.csv)?)?;
            export_report(&data, args)
        }
        Command::ServeWasm(args) => web_app::serve(args.bind, args.open),
    }
}

fn require_csv(csv: &Option<PathBuf>) -> Result<&std::path::Path> {
    let Some(path) = csv.as_deref() else {
        bail!("`--csv <PATH>` is required for export commands");
    };
    Ok(path)
}

fn export_static(data: &PreparedData, args: ExportStaticArgs) -> Result<()> {
    let deployment = render::resolve_deployment(data, args.deployment.as_deref())?;
    fs::create_dir_all(&args.out_dir).with_context(|| {
        format!(
            "failed to create output directory {}",
            args.out_dir.display()
        )
    })?;

    let overview_path = args.out_dir.join(format!(
        "overview_{}.{}",
        args.overview_bucket.slug(),
        args.format.extension()
    ));
    let detail_path = args.out_dir.join(format!(
        "detail_{}.{}",
        slugify(deployment),
        args.format.extension()
    ));
    let hour_path = args
        .out_dir
        .join(format!("hour_heatmap.{}", args.format.extension()));

    match args.format {
        OutputFormat::Svg => {
            fs::write(
                &overview_path,
                render::overview_svg(data, args.overview_bucket)?,
            )
            .with_context(|| format!("failed to write {}", overview_path.display()))?;
            fs::write(&detail_path, render::detail_svg(data, deployment)?)
                .with_context(|| format!("failed to write {}", detail_path.display()))?;
            fs::write(&hour_path, render::hour_heatmap_svg(data)?)
                .with_context(|| format!("failed to write {}", hour_path.display()))?;
        }
        OutputFormat::Png => {
            render::overview_chart(data, args.overview_bucket)?.save(&overview_path)?;
            render::detail_chart(data, deployment)?.save(&detail_path)?;
            render::hour_heatmap_chart(data)?.save(&hour_path)?;
        }
    }

    if args.all_details {
        let details_dir = args.out_dir.join("details");
        fs::create_dir_all(&details_dir).with_context(|| {
            format!(
                "failed to create per-deployment detail directory {}",
                details_dir.display()
            )
        })?;

        for summary in &data.deployments {
            let path = details_dir.join(format!(
                "{}.{}",
                slugify(&summary.deployment),
                args.format.extension()
            ));
            render::detail_chart(data, &summary.deployment)?.save(path)?;
        }
    }

    let manifest_path = args.out_dir.join("manifest.txt");
    let manifest = format!(
        "rows={rows}\ndeployments={deployments}\noverview={overview}\ndetail={detail}\nhour={hour}\n",
        rows = format_count(data.events.height()),
        deployments = format_count(data.deployments.len()),
        overview = overview_path.display(),
        detail = detail_path.display(),
        hour = hour_path.display(),
    );
    fs::write(&manifest_path, manifest)
        .with_context(|| format!("failed to write manifest {}", manifest_path.display()))?;

    println!(
        "Wrote static outputs to {}",
        args.out_dir.as_path().display()
    );

    Ok(())
}

fn export_report(data: &PreparedData, args: ExportReportArgs) -> Result<()> {
    let deployment = render::resolve_deployment(data, args.deployment.as_deref())?;
    report::export_report(
        data,
        &args.out,
        args.overview_bucket,
        deployment,
        args.top_details,
    )?;

    println!("Wrote report to {}", args.out.display());
    Ok(())
}
