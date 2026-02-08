//! gardisplay - Display/monitor manager for gardesk.

mod app;
mod config;
mod ui;

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

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,gardisplay=debug")),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("starting gardisplay");

    let config = config::load_config(args.config.as_deref())?;
    let mut app = app::App::new(config, args.demo)?;
    app.run()
}
