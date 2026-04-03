use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::executor::DockerExecutor;
use crate::output::tty::PipelineReporter;
use crate::output::resolve_output_mode;
use crate::pipeline::Pipeline;
use crate::scheduler::Scheduler;

#[derive(Parser)]
#[command(name = "pipelight", version, about = "Lightweight CLI CI/CD tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run a pipeline
    Run {
        /// Path to pipeline config file
        #[arg(short, long, default_value = "pipeline.yml")]
        file: PathBuf,

        /// Run a specific step only
        #[arg(short, long)]
        step: Option<String>,

        /// Dry run - validate and show execution plan without running
        #[arg(long)]
        dry_run: bool,

        /// Output mode: tty, plain, json
        #[arg(long)]
        output: Option<String>,

        /// Run ID for this execution
        #[arg(long)]
        run_id: Option<String>,
    },

    /// Validate pipeline config
    Validate {
        /// Path to pipeline config file
        #[arg(short, long, default_value = "pipeline.yml")]
        file: PathBuf,
    },

    /// List steps in a pipeline
    List {
        /// Path to pipeline config file
        #[arg(short, long, default_value = "pipeline.yml")]
        file: PathBuf,
    },

    /// Retry a failed pipeline run
    Retry {
        /// Run ID of the failed run to retry
        #[arg(long)]
        run_id: String,

        /// Retry only a specific step
        #[arg(long)]
        step: Option<String>,

        /// Output mode: tty, plain, json
        #[arg(long)]
        output: Option<String>,

        /// Path to pipeline config file
        #[arg(short, long, default_value = "pipeline.yml")]
        file: PathBuf,
    },

    /// Show status of a pipeline run
    Status {
        /// Run ID to check
        #[arg(long)]
        run_id: String,

        /// Output mode: tty, plain, json
        #[arg(long)]
        output: Option<String>,
    },
}

pub async fn dispatch(cli: Cli) -> Result<i32> {
    match cli.command {
        Command::Run {
            file,
            step,
            dry_run,
            output,
            run_id,
        } => cmd_run(file, step, dry_run, output, run_id).await,
        Command::Validate { file } => cmd_validate(file).await,
        Command::List { file } => cmd_list(file).await,
        Command::Retry {
            run_id,
            step,
            output,
            file,
        } => {
            let _mode = resolve_output_mode(output);
            // Placeholder: retry logic will be implemented later
            Ok(0)
        }
        Command::Status { run_id, output } => {
            let _mode = resolve_output_mode(output);
            // Placeholder: status logic will be implemented later
            Ok(0)
        }
    }
}

async fn cmd_run(
    file: PathBuf,
    step_filter: Option<String>,
    dry_run: bool,
    output: Option<String>,
    run_id: Option<String>,
) -> Result<i32> {
    let _mode = resolve_output_mode(output);
    let _run_id = run_id;

    let pipeline =
        Pipeline::from_file(&file).context(format!("Failed to load pipeline: {}", file.display()))?;

    let scheduler = Scheduler::new(&pipeline)?;

    if dry_run {
        let reporter = PipelineReporter::new();
        reporter.print_execution_plan(&pipeline, &scheduler);
        return Ok(0);
    }

    let executor = DockerExecutor::new().await?;
    let reporter = PipelineReporter::new();

    reporter.print_header(&pipeline);

    let schedule = scheduler.resolve(step_filter.as_deref())?;

    for batch in &schedule {
        // Each batch contains steps that can run in parallel
        let handles: Vec<_> = batch
            .iter()
            .map(|step_name| {
                let executor = executor.clone();
                let step = pipeline
                    .get_step(step_name)
                    .expect("step must exist")
                    .clone();
                let pipeline_name = pipeline.name.clone();
                tokio::spawn(async move { executor.run_step(&pipeline_name, &step).await })
            })
            .collect();

        for handle in handles {
            let result = handle.await??;
            reporter.print_step_result(&result);
        }
    }

    reporter.print_summary();
    Ok(0)
}

async fn cmd_validate(file: PathBuf) -> Result<i32> {
    let pipeline = Pipeline::from_file(&file)?;
    let _scheduler = Scheduler::new(&pipeline)?;

    let reporter = PipelineReporter::new();
    reporter.print_validation_ok(&pipeline);
    Ok(0)
}

async fn cmd_list(file: PathBuf) -> Result<i32> {
    let pipeline = Pipeline::from_file(&file)?;

    let reporter = PipelineReporter::new();
    reporter.print_step_list(&pipeline);
    Ok(0)
}
