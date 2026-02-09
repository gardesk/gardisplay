//! gardisplay - Display/monitor manager for gardesk.

mod app;
mod config;
mod randr;
mod ui;
mod watchdog;

use std::fs;

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "gardisplay")]
#[command(about = "Display/monitor manager for gardesk")]
struct Args {
    /// Configuration file path
    #[arg(short, long)]
    config: Option<String>,

    /// Demo mode with fake monitors for UI testing
    #[arg(long)]
    demo: bool,
}

/// Get the log file path.
fn log_file_path() -> std::path::PathBuf {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("gardisplay");

    // Ensure directory exists
    let _ = fs::create_dir_all(&cache_dir);

    cache_dir.join("gardisplay.log")
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Set up logging to both console and file
    let log_path = log_file_path();
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file)
        .with_ansi(false);

    let console_layer = tracing_subscriber::fmt::layer();

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,gardisplay=debug")),
        )
        .with(console_layer)
        .with(file_layer)
        .init();

    tracing::info!("starting gardisplay (log file: {:?})", log_path);

    let config = config::load_config(args.config.as_deref())?;
    let mut app = app::App::new(config, args.demo)?;
    app.run()
}
