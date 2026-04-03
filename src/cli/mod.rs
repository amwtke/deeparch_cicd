use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::executor::DockerExecutor;
use crate::output::PipelineReporter;
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
}

pub async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Run {
            file,
            step,
            dry_run,
        } => cmd_run(file, step, dry_run).await,
        Command::Validate { file } => cmd_validate(file).await,
        Command::List { file } => cmd_list(file).await,
    }
}

async fn cmd_run(file: PathBuf, step_filter: Option<String>, dry_run: bool) -> Result<()> {
    let pipeline =
        Pipeline::from_file(&file).context(format!("Failed to load pipeline: {}", file.display()))?;

    let scheduler = Scheduler::new(&pipeline)?;

    if dry_run {
        let reporter = PipelineReporter::new();
        reporter.print_execution_plan(&pipeline, &scheduler);
        return Ok(());
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
    Ok(())
}

async fn cmd_validate(file: PathBuf) -> Result<()> {
    let pipeline = Pipeline::from_file(&file)?;
    let _scheduler = Scheduler::new(&pipeline)?;

    let reporter = PipelineReporter::new();
    reporter.print_validation_ok(&pipeline);
    Ok(())
}

async fn cmd_list(file: PathBuf) -> Result<()> {
    let pipeline = Pipeline::from_file(&file)?;

    let reporter = PipelineReporter::new();
    reporter.print_step_list(&pipeline);
    Ok(())
}
