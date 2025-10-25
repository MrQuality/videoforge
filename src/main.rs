//! CLI entry point launching the Videoforge pipeline.

use clap::Parser;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = videoforge::config::CliArgs::parse();
    let config = videoforge::config::AppConfig::load(cli.clone()).await?;

    if let Err(error) = videoforge::run(config).await {
        tracing::error!(error = %error, "pipeline execution failed");
        return Err(Box::new(error));
    }

    Ok(())
}
