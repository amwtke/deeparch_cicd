mod cli;
mod executor;
mod output;
mod pipeline;
mod scheduler;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("pipelight=info".parse()?))
        .with_target(false)
        .init();

    let cli = Cli::parse();
    cli::dispatch(cli).await
}
