use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::ci::executor::DockerExecutor;
use crate::ci::output::tty::{PipelineProgressUI, PipelineReporter};
use crate::ci::output::{resolve_output_mode, OutputMode};
use crate::ci::output::{json, plain};
use crate::ci::parser::{Pipeline, Strategy};
use crate::run_state::{OnFailureState, PipelineStatus, RunState, StepState, StepStatus};
use crate::ci::scheduler::Scheduler;

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

        /// Skip specific steps (can be specified multiple times)
        #[arg(long, num_args = 1..)]
        skip: Vec<String>,

        /// Dry run - validate and show execution plan without running
        #[arg(long)]
        dry_run: bool,

        /// Output mode: tty, plain, json
        #[arg(long)]
        output: Option<String>,

        /// Run ID for this execution
        #[arg(long)]
        run_id: Option<String>,

        /// Show full container output
        #[arg(long)]
        verbose: bool,
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

        /// Show full container output
        #[arg(long)]
        verbose: bool,
    },

    /// Auto-detect project type and generate pipeline.yml
    Init {
        /// Project directory to scan
        #[arg(short, long, default_value = ".")]
        dir: PathBuf,

        /// Output file path
        #[arg(short, long, default_value = "pipeline.yml")]
        output: PathBuf,
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
            skip,
            dry_run,
            output,
            run_id,
            verbose,
        } => cmd_run(file, step, skip, dry_run, output, run_id, verbose).await,
        Command::Init { dir, output } => cmd_init(dir, output).await,
        Command::Validate { file } => cmd_validate(file).await,
        Command::List { file } => cmd_list(file).await,
        Command::Retry {
            run_id,
            step,
            output,
            file,
            verbose,
        } => {
            let mode = resolve_output_mode(output);
            cmd_retry(run_id, step, mode, file, verbose).await
        }
        Command::Status { run_id, output } => {
            let mode = resolve_output_mode(output);
            cmd_status(run_id, mode).await
        }
    }
}

async fn cmd_run(
    file: PathBuf,
    step_filter: Option<String>,
    skip_steps: Vec<String>,
    dry_run: bool,
    output: Option<String>,
    run_id: Option<String>,
    verbose: bool,
) -> Result<i32> {
    let mode = resolve_output_mode(output);

    let pipeline =
        Pipeline::from_file(&file).context(format!("Failed to load pipeline: {}", file.display()))?;

    // Resolve project directory from pipeline file location
    let project_dir = file.canonicalize()
        .context("Failed to resolve pipeline file path")?
        .parent()
        .expect("pipeline file must have a parent directory")
        .to_path_buf();

    let scheduler = Scheduler::new(&pipeline)?;

    if dry_run {
        if mode == OutputMode::Tty {
            let reporter = PipelineReporter::new();
            reporter.print_execution_plan(&pipeline, &scheduler);
        }
        return Ok(0);
    }

    let run_id = run_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());
    let mut state = RunState::new(&run_id, &pipeline.name);
    let pipeline_start = std::time::Instant::now();

    // Ensure pipelight-misc/ directory exists for artifacts and config
    let misc_dir = project_dir.join("pipelight-misc");
    if !misc_dir.exists() {
        std::fs::create_dir_all(&misc_dir)
            .context("Failed to create pipelight-misc/ directory")?;
    }

    let executor = DockerExecutor::new().await?;

    // Set up progress UI (wrapped in Arc<Mutex> so on_log closure can access it)
    let step_names: Vec<String> = pipeline.steps.iter().map(|s| s.name.clone()).collect();
    let progress: Option<std::sync::Arc<std::sync::Mutex<PipelineProgressUI>>> = if mode == OutputMode::Tty {
        let mut p = PipelineProgressUI::new(&step_names, verbose);
        p.print_header(&pipeline.name, pipeline.steps.len());
        Some(std::sync::Arc::new(std::sync::Mutex::new(p)))
    } else {
        None
    };

    let schedule = scheduler.resolve(step_filter.as_deref())?;

    let mut has_final_failure = false;
    let mut has_retryable_failure = false;
    let mut current_batch_index = 0;
    let mut step_results: Vec<(String, std::time::Duration, bool)> = Vec::new();
    let mut test_summary: Option<crate::ci::builder::test_parser::TestSummary> = None;

    'outer: for (batch_idx, batch) in schedule.iter().enumerate() {
        current_batch_index = batch_idx;

        for step_name in batch {
            // Skip steps specified by --skip
            if skip_steps.contains(step_name) {
                state.add_step(StepState {
                    name: step_name.clone(),
                    status: StepStatus::Skipped,
                    exit_code: None,
                    duration_ms: None,
                    image: pipeline.get_step(step_name).map(|s| s.image.clone()).unwrap_or_default(),
                    command: pipeline.get_step(step_name).map(|s| s.commands.join(" && ")).unwrap_or_default(),
                    stdout: None,
                    stderr: None,
                    error_context: None,
                    on_failure: None,
                    test_summary: None,
                });
                if let Some(ref progress) = progress {
                    progress.lock().unwrap().start_step(step_name);
                    progress.lock().unwrap().finish_step(step_name, true, std::time::Duration::ZERO);
                }
                continue;
            }

            let step = pipeline.get_step(step_name).expect("step must exist").clone();

            // Signal step start
            if let Some(ref progress) = progress {
                progress.lock().unwrap().start_step(step_name);
            }
            if mode == OutputMode::Plain {
                plain::print_step_start(step_name, &step.image);
            }

            // Build on_log callback based on mode
            let sn = step_name.clone();
            let m = mode.clone();
            let v = verbose;
            let progress_ref = progress.clone();
            let result = executor.run_step(&pipeline.name, &step, &project_dir, move |line| {
                match m {
                    OutputMode::Plain => {
                        plain::print_log_line(&sn, line, v);
                    }
                    OutputMode::Tty => {
                        if let Some(ref p) = progress_ref {
                            p.lock().unwrap().update_log(&sn, line);
                        }
                    }
                    _ => {}
                }
            }).await?;

            // Record step result for stats
            step_results.push((step_name.clone(), result.duration, result.success));

            // Finish progress for this step
            if let Some(ref progress) = progress {
                progress.lock().unwrap().finish_step(step_name, result.success, result.duration);
            }
            if mode == OutputMode::Plain {
                plain::print_step_finish(step_name, result.success, result.duration);
            }

            let pipeline_step = pipeline.get_step(step_name);

            // Build on_failure state from pipeline config
            let on_failure_state = pipeline_step
                .and_then(|s| s.on_failure.as_ref())
                .map(|of| OnFailureState {
                    strategy: format!("{:?}", of.strategy).to_lowercase(),
                    max_retries: of.max_retries,
                    retries_remaining: of.max_retries,
                    context_paths: of.context_paths.clone(),
                });

            let step_status = if result.success {
                StepStatus::Success
            } else {
                StepStatus::Failed
            };

            let stdout = result.stdout_string();
            let stderr = result.stderr_string();

            // Parse test output if this is a test step
            let step_test_summary = if step_name == "test" {
                let full_output = format!("{}{}", &stdout, &stderr);
                if let Some(strategy) = crate::ci::builder::strategy_for_pipeline(&pipeline) {
                    let parsed = strategy.parse_test_output(&full_output);
                    if parsed.is_some() {
                        test_summary = parsed.clone();
                    }
                    parsed
                } else {
                    None
                }
            } else {
                None
            };

            state.add_step(StepState {
                name: result.step_name.clone(),
                status: step_status,
                exit_code: Some(result.exit_code),
                duration_ms: Some(result.duration.as_millis() as u64),
                image: pipeline_step.map(|s| s.image.clone()).unwrap_or_default(),
                command: pipeline_step.map(|s| s.commands.join(" && ")).unwrap_or_default(),
                stdout: if stdout.is_empty() { None } else { Some(stdout) },
                stderr: if stderr.is_empty() { None } else { Some(stderr) },
                error_context: None,
                on_failure: on_failure_state,
                test_summary: step_test_summary,
            });

            // Handle failure
            let allow_failure = pipeline_step.map(|s| s.allow_failure).unwrap_or(false);
            if !result.success && !allow_failure {
                let strategy = pipeline_step
                    .and_then(|s| s.on_failure.as_ref())
                    .map(|of| &of.strategy)
                    .unwrap_or(&Strategy::Abort);

                match strategy {
                    Strategy::AutoFix => {
                        has_retryable_failure = true;
                        break 'outer;
                    }
                    Strategy::Abort | Strategy::Notify => {
                        has_final_failure = true;
                        break 'outer;
                    }
                }
            }
        }
    }

    // Mark remaining unexecuted steps as Skipped
    if has_final_failure || has_retryable_failure {
        for remaining_batch in &schedule[current_batch_index + 1..] {
            for step_name in remaining_batch {
                state.add_step(StepState {
                    name: step_name.clone(),
                    status: StepStatus::Skipped,
                    exit_code: None,
                    duration_ms: None,
                    image: pipeline.get_step(step_name).map(|s| s.image.clone()).unwrap_or_default(),
                    command: pipeline.get_step(step_name).map(|s| s.commands.join(" && ")).unwrap_or_default(),
                    stdout: None,
                    stderr: None,
                    error_context: None,
                    on_failure: None,
                    test_summary: None,
                });
            }
        }
    }

    // Set final pipeline status
    state.status = if has_retryable_failure {
        PipelineStatus::Retryable
    } else if has_final_failure {
        PipelineStatus::Failed
    } else {
        PipelineStatus::Success
    };

    state.duration_ms = Some(pipeline_start.elapsed().as_millis() as u64);

    // Save state
    state.save(&RunState::default_base_dir())?;

    // Print stats and test summary
    let total_duration = pipeline_start.elapsed();
    match mode {
        OutputMode::Json => json::print_run_state(&state),
        OutputMode::Plain => {
            if let Some(ref ts) = test_summary {
                plain::print_test_summary(ts);
            }
            plain::print_stats_table(&step_results, total_duration);
        }
        OutputMode::Tty => {
            if let Some(ref progress) = progress {
                let p = progress.lock().unwrap();
                if let Some(ref ts) = test_summary {
                    p.print_test_summary(ts);
                }
                p.print_stats_table(&step_results, total_duration);
            }
        }
    }

    // Exit code
    if has_retryable_failure {
        Ok(1)
    } else if has_final_failure {
        Ok(2)
    } else {
        Ok(0)
    }
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

async fn cmd_retry(
    run_id: String,
    step: Option<String>,
    mode: OutputMode,
    file: PathBuf,
    _verbose: bool,
) -> Result<i32> {
    let step_name = step.ok_or_else(|| anyhow::anyhow!("--step is required for retry command"))?;

    let base = RunState::default_base_dir();
    let mut state = RunState::load(&base, &run_id)?;

    // Validate step exists and is Failed
    {
        let step_state = state
            .get_step(&step_name)
            .ok_or_else(|| anyhow::anyhow!("Step '{}' not found in run '{}'", step_name, run_id))?;

        if step_state.status != StepStatus::Failed {
            anyhow::bail!(
                "Step '{}' is not in Failed status (current: {:?})",
                step_name,
                step_state.status
            );
        }

        // Check retries remaining
        if let Some(ref on_failure) = step_state.on_failure {
            if on_failure.retries_remaining == 0 {
                anyhow::bail!(
                    "Step '{}' has exhausted all retries (max: {})",
                    step_name,
                    on_failure.max_retries
                );
            }
        }
    }

    // Decrement retries
    state.decrement_retries(&step_name);

    // Load pipeline and create executor
    let pipeline = Pipeline::from_file(&file)
        .context(format!("Failed to load pipeline: {}", file.display()))?;
    let project_dir = file.canonicalize()
        .context("Failed to resolve pipeline file path")?
        .parent()
        .expect("pipeline file must have a parent directory")
        .to_path_buf();
    let executor = DockerExecutor::new().await?;

    let pipeline_start = std::time::Instant::now();

    // Re-execute the failed step
    let pipeline_step = pipeline
        .get_step(&step_name)
        .ok_or_else(|| anyhow::anyhow!("Step '{}' not found in pipeline config", step_name))?;

    let result = executor.run_step(&pipeline.name, pipeline_step, &project_dir, |_| {}).await?;

    // Update step state
    {
        let ss = state
            .get_step_mut(&step_name)
            .expect("step must exist in state");
        ss.status = if result.success {
            StepStatus::Success
        } else {
            StepStatus::Failed
        };
        ss.exit_code = Some(result.exit_code);
        ss.duration_ms = Some(result.duration.as_millis() as u64);
        let stdout = result.stdout_string();
        let stderr = result.stderr_string();
        ss.stdout = if stdout.is_empty() { None } else { Some(stdout) };
        ss.stderr = if stderr.is_empty() { None } else { Some(stderr) };
    }

    // If retried step succeeded, run downstream Skipped steps
    if result.success {
        // Collect skipped step names in order
        let skipped_names: Vec<String> = state
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Skipped)
            .map(|s| s.name.clone())
            .collect();

        for skipped_name in &skipped_names {
            // Check if all dependencies are Success
            let ps = pipeline.get_step(skipped_name);
            let deps_satisfied = ps
                .map(|s| {
                    s.depends_on.iter().all(|dep| {
                        state
                            .get_step(dep)
                            .map(|d| d.status == StepStatus::Success)
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(true);

            if !deps_satisfied {
                continue;
            }

            // Execute the skipped step
            let skipped_step = match pipeline.get_step(skipped_name) {
                Some(s) => s,
                None => continue,
            };

            let sr = executor.run_step(&pipeline.name, skipped_step, &project_dir, |_| {}).await?;

            // Update state
            {
                let ss = state
                    .get_step_mut(skipped_name)
                    .expect("step must exist in state");
                ss.status = if sr.success {
                    StepStatus::Success
                } else {
                    StepStatus::Failed
                };
                ss.exit_code = Some(sr.exit_code);
                ss.duration_ms = Some(sr.duration.as_millis() as u64);
                let stdout = sr.stdout_string();
                let stderr = sr.stderr_string();
                ss.stdout = if stdout.is_empty() { None } else { Some(stdout) };
                ss.stderr = if stderr.is_empty() { None } else { Some(stderr) };
            }

            // If this step failed, stop based on its strategy
            if !sr.success {
                let allow_failure = skipped_step.allow_failure;
                if !allow_failure {
                    break;
                }
            }
        }
    }

    // Determine overall status
    let all_success = state.steps.iter().all(|s| s.status == StepStatus::Success);
    let has_retryable = state.steps.iter().any(|s| {
        s.status == StepStatus::Failed
            && s.on_failure
                .as_ref()
                .map(|of| of.strategy == "auto_fix" && of.retries_remaining > 0)
                .unwrap_or(false)
    });

    state.status = if all_success {
        PipelineStatus::Success
    } else if has_retryable {
        PipelineStatus::Retryable
    } else {
        PipelineStatus::Failed
    };

    state.duration_ms = Some(pipeline_start.elapsed().as_millis() as u64);

    // Save state
    state.save(&base)?;

    // Output based on mode
    match mode {
        OutputMode::Json => json::print_run_state(&state),
        OutputMode::Plain | OutputMode::Tty => plain::print_run_state(&state),
    }

    // Exit code
    match state.status {
        PipelineStatus::Success => Ok(0),
        PipelineStatus::Retryable => Ok(1),
        _ => Ok(2),
    }
}

async fn cmd_init(dir: PathBuf, output_path: PathBuf) -> Result<i32> {
    use crate::ci::detector;

    let (info, pipeline) = detector::detect_and_generate(&dir)?;

    // Print detection results
    println!("Detected project: {}", info.project_type);
    if let Some(ref subdir) = info.subdir {
        println!("Detected in subdirectory: {}/", subdir);
    }
    if let Some(ref ver) = info.language_version {
        println!("Language version: {}", ver);
    }
    if let Some(ref fw) = info.framework {
        println!("Framework: {}", fw);
    }
    println!("Docker image: {}", info.image);
    println!("Steps: {}", pipeline.steps.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "));

    for warning in &info.warnings {
        eprintln!("WARNING: {}", warning);
    }

    // Serialize to YAML
    let yaml = serde_yaml::to_string(&pipeline)
        .context("Failed to serialize pipeline to YAML")?;

    // Write file
    std::fs::write(&output_path, &yaml)
        .context(format!("Failed to write {}", output_path.display()))?;

    println!("\nGenerated: {}", output_path.display());
    Ok(0)
}

async fn cmd_status(run_id: String, mode: OutputMode) -> Result<i32> {
    let base = RunState::default_base_dir();
    let state = RunState::load(&base, &run_id)?;

    match mode {
        OutputMode::Json => json::print_run_state(&state),
        OutputMode::Plain | OutputMode::Tty => plain::print_run_state(&state),
    }

    Ok(0)
}
