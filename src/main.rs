mod cli;
mod detector;
mod executor;
mod output;
mod pipeline;
mod run_state;
mod scheduler;

use clap::Parser;

use cli::Cli;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("pipelight=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match cli::dispatch(cli).await {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("Error: {:#}", e);
            std::process::exit(2);
        }
    }
}
