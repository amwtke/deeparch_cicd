mod ci;
mod cli;
mod run_state;

use clap::Parser;

use cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Suppress tracing output in TTY mode to avoid interfering with progress UI.
    // In plain/json mode or when RUST_LOG is set, show tracing output.
    let show_tracing = std::env::var("RUST_LOG").is_ok() || !atty::is(atty::Stream::Stdout);
    if show_tracing {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("pipelight=info".parse().unwrap()),
            )
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter("pipelight=warn")
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    }

    match cli::dispatch(cli).await {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("Error: {:#}", e);
            std::process::exit(2);
        }
    }
}
