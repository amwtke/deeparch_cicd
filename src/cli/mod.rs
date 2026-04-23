use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::ci::callback::action::CallbackCommandAction;
use crate::ci::callback::command::CallbackCommandRegistry;
use crate::ci::executor::DockerExecutor;
use crate::ci::output::tty::{PipelineProgressUI, PipelineReporter};
use crate::ci::output::{json, plain};
use crate::ci::output::{resolve_output_mode, OutputMode};
use crate::ci::parser::{Pipeline, Step};
use crate::ci::scheduler::Scheduler;
use crate::run_state::{OnFailureState, PipelineStatus, RunState, StepState, StepStatus};

#[derive(Parser)]
#[command(name = "pipelight", version, about = "Lightweight CLI CI/CD tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// List all detected steps for the current project
    #[arg(long)]
    pub list_steps: bool,

    /// Project directory (used with --list-steps)
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,
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

        /// Enable ping-pong communication test step
        #[arg(long)]
        ping_pong: bool,

        /// Force full-scan + report-only mode for lint/scan steps (e.g. PMD).
        /// Bypasses incremental git-diff scanning; violations do not trigger auto_fix.
        #[arg(long = "full-report-only")]
        full_report_only: bool,

        /// Use the given remote ref (e.g. `origin/main`) as the branch-ahead base
        /// for the git-diff step, replacing `@{upstream}`. Lets incremental
        /// code-quality scans cover ALL files changed since the branch was cut
        /// from a mainline branch. If the ref is not present locally, the pipeline
        /// exits with a RuntimeError asking you to `git fetch` first.
        #[arg(long = "git-diff-from-remote-branch", value_name = "REMOTE_BRANCH")]
        git_diff_from_remote_branch: Option<String>,
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

        /// Override the branch-ahead base ref used by the git-diff step. If omitted
        /// on retry, the base ref persisted in the run state from the original run
        /// is reused.
        #[arg(long = "git-diff-from-remote-branch", value_name = "REMOTE_BRANCH")]
        git_diff_from_remote_branch: Option<String>,
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

    /// Clean all pipelight-generated artifacts (pipeline.yml, pipelight-misc/)
    Clean {
        /// Project directory to clean
        #[arg(short, long, default_value = ".")]
        dir: PathBuf,
    },

    /// Pull all Docker images required by pipeline.yml
    DockerPrepare {
        /// Path to pipeline config file
        #[arg(short, long, default_value = "pipeline.yml")]
        file: PathBuf,
    },
}

pub async fn dispatch(cli: Cli) -> Result<i32> {
    if cli.list_steps {
        return cmd_list_steps(cli.dir).await;
    }

    let command = cli.command.ok_or_else(|| {
        anyhow::anyhow!(
            "No subcommand provided. Use --help for usage or --list-steps to list project steps."
        )
    })?;

    match command {
        Command::Run {
            file,
            step,
            skip,
            dry_run,
            output,
            run_id,
            verbose,
            ping_pong,
            full_report_only,
            git_diff_from_remote_branch,
        } => {
            cmd_run(
                file,
                step,
                skip,
                dry_run,
                output,
                run_id,
                verbose,
                ping_pong,
                full_report_only,
                git_diff_from_remote_branch,
            )
            .await
        }
        Command::Init { dir, output } => cmd_init(dir, output).await,
        Command::Validate { file } => cmd_validate(file).await,
        Command::List { file } => cmd_list(file).await,
        Command::Retry {
            run_id,
            step,
            output,
            file,
            verbose,
            git_diff_from_remote_branch,
        } => {
            let mode = resolve_output_mode(output);
            cmd_retry(
                run_id,
                step,
                mode,
                file,
                verbose,
                git_diff_from_remote_branch,
            )
            .await
        }
        Command::Status { run_id, output } => {
            let mode = resolve_output_mode(output);
            cmd_status(run_id, mode).await
        }
        Command::Clean { dir } => cmd_clean(dir).await,
        Command::DockerPrepare { file } => cmd_docker_prepare(file).await,
    }
}

/// Conservative ASCII whitelist for a git ref value supplied via
/// `--git-diff-from-remote-branch`. Permits only alphanumerics plus
/// `/`, `_`, `.`, `-` — sufficient for common refs like `origin/main`
/// or `origin/release/v1.2.3`, and blocks shell metacharacters that
/// would otherwise break out of the `format!`-interpolated script
/// (quotes, backticks, `$`, `\`, `;`, etc.).
pub(crate) fn is_safe_ref(r: &str) -> bool {
    !r.is_empty()
        && r.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_.-".contains(c))
}

#[allow(clippy::too_many_arguments)]
async fn cmd_run(
    file: PathBuf,
    step_filter: Option<String>,
    skip_steps: Vec<String>,
    dry_run: bool,
    output: Option<String>,
    run_id: Option<String>,
    verbose: bool,
    ping_pong: bool,
    full_report_only: bool,
    git_diff_from_remote_branch: Option<String>,
) -> Result<i32> {
    let mode = resolve_output_mode(output);

    let mut pipeline = Pipeline::from_file(&file)
        .context(format!("Failed to load pipeline: {}", file.display()))?;

    // Activate ping-pong step if --ping-pong flag is set
    if ping_pong {
        if let Some(step) = pipeline.steps.iter_mut().find(|s| s.name == "ping-pong") {
            step.active = true;
        }
    }

    // Tag-based activation: --full-report-only flips the "full" / "non-full"
    // activation groups. Steps with empty/"default" tag are untouched.
    for step in pipeline.steps.iter_mut() {
        match step.tag.as_str() {
            "full" => step.active = full_report_only,
            "non-full" => step.active = !full_report_only,
            _ => {}
        }
    }

    // If --git-diff-from-remote-branch is set, validate the ref value then
    // overwrite the git-diff step's script with the literal-base-ref variant.
    // Validation is a conservative ASCII whitelist (alnum + `/ _ . -`) —
    // sufficient for `origin/main`, `origin/release/v1.2`, etc., and rejects
    // chars like `"`, `` ` ``, `$`, `\` that would break shell quoting.
    if let Some(ref base) = git_diff_from_remote_branch {
        if !is_safe_ref(base) {
            anyhow::bail!(
                "--git-diff-from-remote-branch: invalid ref '{}' — must contain only ASCII alphanumerics and /_.-",
                base
            );
        }
        if let Some(step) = pipeline.steps.iter_mut().find(|s| s.name == "git-diff") {
            use crate::ci::pipeline_builder::StepDef;
            let gd =
                crate::ci::pipeline_builder::base::GitDiffStep::with_base_ref(Some(base.clone()));
            step.commands = gd.config().commands;
            step.on_failure = Some(gd.exception_mapping().to_on_failure());
        }
    }

    // Resolve project directory from pipeline file location
    let project_dir = file
        .canonicalize()
        .context("Failed to resolve pipeline file path")?
        .parent()
        .expect("pipeline file must have a parent directory")
        .to_path_buf();

    let scheduler = Scheduler::new(&pipeline)?;
    let registry = CallbackCommandRegistry::new();
    let step_def_map = crate::ci::pipeline_builder::step_defs_for_pipeline(&pipeline);

    if dry_run {
        if mode == OutputMode::Tty {
            let reporter = PipelineReporter::new();
            reporter.print_execution_plan(&pipeline, &scheduler);
        }
        return Ok(0);
    }

    let run_id = run_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());
    let mut state = RunState::new(&run_id, &pipeline.name);
    let state_base = RunState::default_base_dir();
    state.full_report_only = full_report_only;
    state.git_diff_base = git_diff_from_remote_branch.clone();
    let pipeline_start = std::time::Instant::now();

    // Clear logs and report directories in pipelight-misc/, but preserve config files
    // (e.g. pmd-ruleset.xml, spotbugs-exclude.xml) to avoid expensive regeneration.
    let misc_dir = project_dir.join("pipelight-misc");
    if misc_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&misc_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                // Remove log files, report directories, and counter files; keep everything else (rulesets, exclude filters)
                if name_str.ends_with(".log")
                    || name_str.ends_with("-report")
                    || name_str.ends_with("-counter")
                {
                    if path.is_dir() {
                        let _ = std::fs::remove_dir_all(&path);
                    } else {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
    }
    std::fs::create_dir_all(&misc_dir).context("Failed to create pipelight-misc/ directory")?;

    let executor = DockerExecutor::new().await?;

    let schedule = scheduler.resolve(step_filter.as_deref())?;

    // Build step names in DAG execution order (flattened from schedule batches)
    let step_names: Vec<String> = schedule.iter().flatten().cloned().collect();

    // Set up progress UI (wrapped in Arc<Mutex> so on_log closure can access it)
    let progress: Option<std::sync::Arc<std::sync::Mutex<PipelineProgressUI>>> =
        if mode == OutputMode::Tty {
            let mut p = PipelineProgressUI::new(&step_names, verbose);
            p.set_batches(&schedule);
            p.print_header(&pipeline.name, pipeline.steps.len());
            Some(std::sync::Arc::new(std::sync::Mutex::new(p)))
        } else {
            None
        };

    let mut has_final_failure = false;
    let mut has_retryable_failure = false;
    let mut current_batch_index = 0;
    let mut step_results: Vec<(String, std::time::Duration, bool, String)> = Vec::new();
    let mut test_summary: Option<crate::ci::pipeline_builder::test_parser::TestSummary> = None;

    'outer: for (batch_idx, batch) in schedule.iter().enumerate() {
        current_batch_index = batch_idx;

        for step_name in batch {
            // Skip inactive steps or steps specified by --skip
            let is_inactive = pipeline
                .get_step(step_name)
                .map(|s| !s.active)
                .unwrap_or(false);
            if is_inactive || skip_steps.contains(step_name) {
                state.add_step(StepState {
                    name: step_name.clone(),
                    status: StepStatus::Skipped,
                    exit_code: None,
                    duration_ms: None,
                    image: pipeline
                        .get_step(step_name)
                        .map(|s| s.image.clone())
                        .unwrap_or_default(),
                    command: pipeline
                        .get_step(step_name)
                        .map(|s| s.commands.join(" && "))
                        .unwrap_or_default(),
                    stdout: None,
                    stderr: None,
                    error_context: None,
                    on_failure: None,
                    test_summary: None,
                    report_summary: None,
                    report_path: None,
                });
                let _ = state.save(&state_base);
                if let Some(ref progress) = progress {
                    progress.lock().unwrap().start_step(step_name);
                    progress.lock().unwrap().finish_step(
                        step_name,
                        true,
                        std::time::Duration::ZERO,
                    );
                }
                continue;
            }

            let mut step = pipeline
                .get_step(step_name)
                .expect("step must exist")
                .clone();

            // Inject git credentials into git-pull step if configured
            if step_name == "git-pull" {
                if let Some(ref creds) = pipeline.git_credentials {
                    step.env
                        .insert("GIT_PIPELIGHT_USER".into(), creds.username.clone());
                    step.env
                        .insert("GIT_PIPELIGHT_PASS".into(), creds.password.clone());
                }
            }

            // Propagate --full-report-only into every step as env var; scan steps (e.g. PMD)
            // read PIPELIGHT_FULL_REPORT_ONLY to switch into full-scan + report-only mode.
            if full_report_only {
                step.env
                    .insert("PIPELIGHT_FULL_REPORT_ONLY".into(), "1".into());
            }

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
            let on_log = move |line: &crate::ci::executor::LogLine| match m {
                OutputMode::Plain => {
                    plain::print_log_line(&sn, line, v);
                }
                OutputMode::Tty => {
                    if let Some(ref p) = progress_ref {
                        p.lock().unwrap().update_log(&sn, line);
                    }
                }
                _ => {}
            };
            let result = if step.local {
                DockerExecutor::run_step_local(&step, &project_dir, on_log).await?
            } else {
                executor
                    .run_step(&pipeline.name, &step, &project_dir, on_log)
                    .await?
            };

            let pipeline_step = pipeline.get_step(step_name);

            let step_status = if result.success {
                StepStatus::Success
            } else {
                StepStatus::Failed
            };

            let stdout = result.stdout_string();
            let stderr = result.stderr_string();

            // Generate report: summary string + write log file
            let (report_summary, report_path_str) = generate_step_report(
                &pipeline,
                &project_dir,
                &misc_dir,
                step_name,
                result.success,
                &stdout,
                &stderr,
            );
            // Record step result for stats
            step_results.push((
                step_name.clone(),
                result.duration,
                result.success,
                report_summary.clone(),
            ));

            // Finish progress for this step with report info
            if let Some(ref progress) = progress {
                progress.lock().unwrap().finish_step_with_report(
                    step_name,
                    result.success,
                    result.duration,
                    Some(&report_summary),
                    Some(&report_path_str),
                );
            }
            if mode == OutputMode::Plain {
                plain::print_step_report(
                    step_name,
                    result.success,
                    result.duration,
                    &report_summary,
                    &report_path_str,
                );
            }

            // Build on_failure state: prefer runtime exception resolve, fall back to YAML config.
            // NOTE: we key on exit_code != 0 (not !success) so that `allow_failure` steps —
            // which are marked success even when the underlying build failed — still publish
            // a callback (e.g. `test_print`) that the LLM can act on.
            let mut on_failure_state = if result.exit_code != 0 {
                // Try runtime resolve via StepDef exception_mapping
                if let Some(ref defs) = step_def_map {
                    if let Some(sd) = defs.get(step_name) {
                        let mapping = sd.exception_mapping();
                        let match_fn = |ec: i64, out: &str, err: &str| -> Option<String> {
                            sd.match_exception(ec, out, err)
                        };
                        let resolved =
                            mapping.resolve(result.exit_code, &stdout, &stderr, Some(&match_fn));
                        let action = registry.action_for(&resolved.command);
                        Some(OnFailureState {
                            exception_key: resolved.exception_key,
                            command: resolved.command,
                            action,
                            max_retries: resolved.max_retries,
                            retries_remaining: resolved.max_retries,
                            context_paths: resolved.context_paths,
                        })
                    } else {
                        // No StepDef for this step, fall back to YAML
                        pipeline_step.and_then(|s| s.on_failure.as_ref()).map(|of| {
                            let action = registry.action_for(&of.callback_command);
                            OnFailureState {
                                exception_key: "yaml_configured".into(),
                                command: of.callback_command.clone(),
                                action,
                                max_retries: of.max_retries,
                                retries_remaining: of.max_retries,
                                context_paths: of.context_paths.clone(),
                            }
                        })
                    }
                } else {
                    // No strategy found, fall back to YAML
                    pipeline_step.and_then(|s| s.on_failure.as_ref()).map(|of| {
                        let action = registry.action_for(&of.callback_command);
                        OnFailureState {
                            exception_key: "yaml_configured".into(),
                            command: of.callback_command.clone(),
                            action,
                            max_retries: of.max_retries,
                            retries_remaining: of.max_retries,
                            context_paths: of.context_paths.clone(),
                        }
                    })
                }
            } else {
                // Step succeeded — try runtime resolver first so steps like `test`
                // can still emit a report-only callback (e.g. `test_print_command`)
                // to have the LLM print a per-module summary table. Fall back to
                // the YAML-configured on_failure for JSON output completeness.
                let runtime_state = if let Some(ref defs) = step_def_map {
                    defs.get(step_name).and_then(|sd| {
                        let match_fn = |ec: i64, out: &str, err: &str| -> Option<String> {
                            sd.match_exception(ec, out, err)
                        };
                        // Only surface a callback when match_exception returned a key —
                        // otherwise the default (Abort/RuntimeError) is meaningless noise.
                        match_fn(result.exit_code, &stdout, &stderr).map(|_| {
                            let resolved = sd.exception_mapping().resolve(
                                result.exit_code,
                                &stdout,
                                &stderr,
                                Some(&match_fn),
                            );
                            let action = registry.action_for(&resolved.command);
                            OnFailureState {
                                exception_key: resolved.exception_key,
                                command: resolved.command,
                                action,
                                max_retries: resolved.max_retries,
                                retries_remaining: resolved.max_retries,
                                context_paths: resolved.context_paths,
                            }
                        })
                    })
                } else {
                    None
                };
                runtime_state.or_else(|| {
                    pipeline_step.and_then(|s| s.on_failure.as_ref()).map(|of| {
                        let action = registry.action_for(&of.callback_command);
                        OnFailureState {
                            exception_key: "yaml_configured".into(),
                            command: of.callback_command.clone(),
                            action,
                            max_retries: of.max_retries,
                            retries_remaining: of.max_retries,
                            context_paths: of.context_paths.clone(),
                        }
                    })
                })
            };

            // Parse test output for backward-compat test_summary field
            let step_test_summary = parse_step_test_summary(&pipeline, step_name, &stdout, &stderr);
            if step_test_summary.is_some() {
                test_summary = step_test_summary.clone();
            }

            state.add_step(StepState {
                name: result.step_name.clone(),
                status: step_status,
                exit_code: Some(result.exit_code),
                duration_ms: Some(result.duration.as_millis() as u64),
                image: pipeline_step.map(|s| s.image.clone()).unwrap_or_default(),
                command: pipeline_step
                    .map(|s| s.commands.join(" && "))
                    .unwrap_or_default(),
                stdout: if stdout.is_empty() {
                    None
                } else {
                    Some(stdout)
                },
                stderr: if stderr.is_empty() {
                    None
                } else {
                    Some(stderr)
                },
                error_context: None,
                on_failure: on_failure_state.clone(),
                test_summary: step_test_summary,
                report_summary: Some(report_summary),
                report_path: Some(report_path_str),
            });
            let _ = state.save(&state_base);

            // Handle failure
            let allow_failure = pipeline_step.map(|s| s.allow_failure).unwrap_or(false);
            if !result.success && !allow_failure {
                if let Some(ref mut ofs) = on_failure_state {
                    match ofs.action {
                        CallbackCommandAction::Skip => {
                            // Mark this step as skipped and continue pipeline
                            if let Some(last) = state.steps.last_mut() {
                                last.status = StepStatus::Skipped;
                            }
                            continue;
                        }
                        CallbackCommandAction::Retry if ofs.max_retries > 0 => {
                            has_retryable_failure = true;
                            break 'outer;
                        }
                        _ => {
                            has_final_failure = true;
                            break 'outer;
                        }
                    }
                } else {
                    has_final_failure = true;
                    break 'outer;
                }
            }
        }
    }

    // Mark remaining unexecuted steps as Skipped
    if has_final_failure || has_retryable_failure {
        state.mark_unexecuted_as_skipped(&schedule, current_batch_index, |name| {
            let image = pipeline
                .get_step(name)
                .map(|s| s.image.clone())
                .unwrap_or_default();
            let command = pipeline
                .get_step(name)
                .map(|s| s.commands.join(" && "))
                .unwrap_or_default();
            (image, command)
        });
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

/// Decide whether a Skipped step should be re-executed by the retry cascade.
///
/// Three distinct situations end up with `StepStatus::Skipped` in run state:
/// 1. Pipeline config has `active: false` (e.g. ping-pong) — never meant to run
///    without an explicit opt-in, must stay skipped.
/// 2. Step actually ran, failed, and its `on_failure` action was `Skip`
///    (e.g. git-pull with `git_fail`) — the skip was a deliberate outcome,
///    re-running would just reproduce the same failure.
/// 3. Step never ran because an upstream step was failing/retryable — this
///    is the ONLY case the cascade exists for: now that the upstream has
///    been fixed, the step deserves a chance.
///
/// Case 2 is distinguishable by `exit_code.is_some()` (the step actually
/// executed and produced an exit code), case 1 by the pipeline config's
/// `active` flag, and case 3 by the absence of both signals.
fn is_cascadable_skipped(step_state: &StepState, pipeline_step: Option<&Step>) -> bool {
    if step_state.status != StepStatus::Skipped {
        return false;
    }
    // Case 1: inactive in pipeline config
    if let Some(ps) = pipeline_step {
        if !ps.active {
            return false;
        }
    }
    // Case 2: step actually executed and was skipped by failure policy
    if step_state.exit_code.is_some() {
        return false;
    }
    // Case 3: genuinely held back by upstream — cascade eligible
    true
}

/// Compute `report_summary` and persist the step log file, returning the
/// summary string and the project-relative report path.
///
/// Used by both `cmd_run` and `cmd_retry` so retry paths populate the same
/// JSON fields (`report_summary`, `report_path`) that the initial run does.
fn generate_step_report(
    pipeline: &Pipeline,
    project_dir: &std::path::Path,
    misc_dir: &std::path::Path,
    step_name: &str,
    success: bool,
    stdout: &str,
    stderr: &str,
) -> (String, String) {
    let report_summary = crate::ci::pipeline_builder::step_defs_for_pipeline(pipeline)
        .and_then(|map| {
            map.get(step_name)
                .map(|sd| sd.output_report_str(success, stdout, stderr))
        })
        .unwrap_or_else(|| {
            crate::ci::pipeline_builder::base::BaseStrategy::default_report_str(
                step_name, success, stdout, stderr,
            )
        });

    let report_log_path =
        crate::ci::pipeline_builder::write_step_report(misc_dir, step_name, stdout, stderr);
    let report_path_str = report_log_path
        .strip_prefix(project_dir)
        .unwrap_or(&report_log_path)
        .to_string_lossy()
        .to_string();

    (report_summary, report_path_str)
}

/// Parse `test_summary` from a step's combined output when the step is the
/// `test` step and a pipeline strategy is available. Returns `None` for any
/// other step name or when the strategy cannot extract a summary.
fn parse_step_test_summary(
    pipeline: &Pipeline,
    step_name: &str,
    stdout: &str,
    stderr: &str,
) -> Option<crate::ci::pipeline_builder::test_parser::TestSummary> {
    if step_name != "test" {
        return None;
    }
    let strategy = crate::ci::pipeline_builder::strategy_for_pipeline(pipeline)?;
    let full_output = format!("{}{}", stdout, stderr);
    strategy.parse_test_output(&full_output)
}

/// Whether `dep_name` counts as "satisfied" for the retry cascade.
///
/// A dep satisfies downstream iff it succeeded OR it's an inactive step that
/// was intentionally skipped (e.g. `pmd_full` when non-full PMD ran). Without
/// the inactive-Skipped carve-out, steps like `test` that depend on a tag
/// variant would never cascade on retry.
fn cascade_dep_satisfied(state: &RunState, pipeline: &Pipeline, dep_name: &str) -> bool {
    match (state.get_step(dep_name), pipeline.get_step(dep_name)) {
        (Some(d), Some(c)) => {
            d.status == StepStatus::Success || (!c.active && d.status == StepStatus::Skipped)
        }
        _ => false,
    }
}

/// Re-resolve a step's `on_failure` callback based on the latest stdout/stderr
/// and carry over the already-decremented `retries_remaining`.
///
/// Used by `cmd_retry` so callbacks reflect the current run, not the frozen
/// state from the initial failure. Returns `None` when the step has no
/// `StepDef` or the step succeeded with no `match_exception` marker.
fn reresolve_on_failure(
    sd: Option<&dyn crate::ci::pipeline_builder::StepDef>,
    exit_code: i64,
    stdout: &str,
    stderr: &str,
    prev_remaining: Option<u32>,
    registry: &CallbackCommandRegistry,
) -> Option<OnFailureState> {
    let sd = sd?;
    let match_fn =
        |ec: i64, out: &str, err: &str| -> Option<String> { sd.match_exception(ec, out, err) };

    // On success, only surface a callback when match_exception produced a key
    // (e.g. test_print on a passing test step). Otherwise return None.
    if exit_code == 0 && match_fn(exit_code, stdout, stderr).is_none() {
        return None;
    }

    let resolved = sd
        .exception_mapping()
        .resolve(exit_code, stdout, stderr, Some(&match_fn));
    let action = registry.action_for(&resolved.command);
    let mut of = OnFailureState {
        exception_key: resolved.exception_key,
        command: resolved.command,
        action,
        max_retries: resolved.max_retries,
        retries_remaining: resolved.max_retries,
        context_paths: resolved.context_paths,
    };
    if let Some(rem) = prev_remaining {
        of.retries_remaining = rem.min(of.max_retries);
    }
    Some(of)
}

async fn cmd_retry(
    run_id: String,
    step: Option<String>,
    mode: OutputMode,
    file: PathBuf,
    _verbose: bool,
    git_diff_from_remote_branch_override: Option<String>,
) -> Result<i32> {
    let step_name = step.ok_or_else(|| anyhow::anyhow!("--step is required for retry command"))?;

    let base = RunState::default_base_dir();
    let mut state = RunState::load(&base, &run_id)?;

    // If the user passed an explicit override on the retry command, validate it
    // through the same whitelist `cmd_run` uses; otherwise reuse the value
    // persisted in run_state from the original run.
    if let Some(ref override_base) = git_diff_from_remote_branch_override {
        if !is_safe_ref(override_base) {
            anyhow::bail!(
                "--git-diff-from-remote-branch: invalid ref '{}' — must contain only ASCII alphanumerics and /_.-",
                override_base
            );
        }
        state.git_diff_base = Some(override_base.clone());
        // Persist immediately so the new base ref is durable even if a later
        // step in cmd_retry errors out before reaching the post-execution save.
        state.save(&base)?;
    }
    let effective_git_diff_base = state.git_diff_base.clone();

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
    let project_dir = file
        .canonicalize()
        .context("Failed to resolve pipeline file path")?
        .parent()
        .expect("pipeline file must have a parent directory")
        .to_path_buf();
    let executor = DockerExecutor::new().await?;

    let pipeline_start = std::time::Instant::now();

    // Re-execute the failed step
    let mut retry_step = pipeline
        .get_step(&step_name)
        .ok_or_else(|| anyhow::anyhow!("Step '{}' not found in pipeline config", step_name))?
        .clone();

    // If retrying the git-diff step and a base ref is in effect (either from
    // this invocation's override or persisted from the original run), rewrite
    // the step's commands to use the literal-base-ref variant. Downstream
    // quality steps need no rewrite because they just read `diff.txt`.
    if retry_step.name == "git-diff" {
        if let Some(ref base) = effective_git_diff_base {
            use crate::ci::pipeline_builder::StepDef;
            let gd =
                crate::ci::pipeline_builder::base::GitDiffStep::with_base_ref(Some(base.clone()));
            retry_step.commands = gd.config().commands;
            retry_step.on_failure = Some(gd.exception_mapping().to_on_failure());
        }
    }

    // Inject git credentials if configured
    if step_name == "git-pull" {
        if let Some(ref creds) = pipeline.git_credentials {
            retry_step
                .env
                .insert("GIT_PIPELIGHT_USER".into(), creds.username.clone());
            retry_step
                .env
                .insert("GIT_PIPELIGHT_PASS".into(), creds.password.clone());
        }
    }

    // Inherit --full-report-only from the original run so retries keep the same scan semantics.
    if state.full_report_only {
        retry_step
            .env
            .insert("PIPELIGHT_FULL_REPORT_ONLY".into(), "1".into());
    }

    let result = if retry_step.local {
        DockerExecutor::run_step_local(&retry_step, &project_dir, |_| {}).await?
    } else {
        executor
            .run_step(&pipeline.name, &retry_step, &project_dir, |_| {})
            .await?
    };

    // Ensure misc dir exists for log persistence (retry may run before any
    // prior cmd_run on this machine, e.g. manual retry against a loaded state).
    let misc_dir = project_dir.join("pipelight-misc");
    let _ = std::fs::create_dir_all(&misc_dir);

    // Callback registry + StepDef map are shared by the main-path state
    // update (below) AND the cascade loop further down. Both paths need
    // to call `reresolve_on_failure` so cascaded steps also publish their
    // callbacks (test_print, auto_gen_jacoco_config, etc.) — not just the
    // primary retried step.
    let callback_registry = CallbackCommandRegistry::new();
    let step_def_map = crate::ci::pipeline_builder::step_defs_for_pipeline(&pipeline);

    // Update step state
    {
        let stdout = result.stdout_string();
        let stderr = result.stderr_string();
        let (report_summary, report_path_str) = generate_step_report(
            &pipeline,
            &project_dir,
            &misc_dir,
            &step_name,
            result.success,
            &stdout,
            &stderr,
        );
        let step_test_summary = parse_step_test_summary(&pipeline, &step_name, &stdout, &stderr);

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
        ss.stdout = if stdout.is_empty() {
            None
        } else {
            Some(stdout.clone())
        };
        ss.stderr = if stderr.is_empty() {
            None
        } else {
            Some(stderr.clone())
        };
        ss.report_summary = Some(report_summary);
        ss.report_path = Some(report_path_str);
        if step_test_summary.is_some() {
            ss.test_summary = step_test_summary;
        }

        // Re-resolve on_failure so the callback reflects THIS run's stdout/stderr,
        // not the state frozen from the initial failure. Preserve retries_remaining
        // (already decremented above); only refresh the other fields.
        let sd = step_def_map.as_ref().and_then(|defs| defs.get(&step_name));
        let prev_remaining = state
            .get_step(&step_name)
            .and_then(|s| s.on_failure.as_ref())
            .map(|of| of.retries_remaining);
        let new_on_failure = reresolve_on_failure(
            sd.map(|b| b.as_ref()),
            result.exit_code,
            &stdout,
            &stderr,
            prev_remaining,
            &callback_registry,
        );
        let ss = state
            .get_step_mut(&step_name)
            .expect("step must exist in state");
        ss.on_failure = new_on_failure;
    }
    let _ = state.save(&base);

    // If retried step succeeded, run downstream Skipped steps.
    // Only cascade to steps that are truly "held back by upstream" — exclude
    // inactive steps (e.g. ping-pong) and steps that already ran and were
    // skipped by their on_failure Skip policy (e.g. git-pull on git_fail).
    // See `is_cascadable_skipped` for details.
    if result.success {
        let skipped_names: Vec<String> = state
            .steps
            .iter()
            .filter(|s| is_cascadable_skipped(s, pipeline.get_step(&s.name)))
            .map(|s| s.name.clone())
            .collect();

        for skipped_name in &skipped_names {
            // Check if all dependencies are Success
            let ps = pipeline.get_step(skipped_name);
            let deps_satisfied = ps
                .map(|s| {
                    s.depends_on
                        .iter()
                        .all(|dep| cascade_dep_satisfied(&state, &pipeline, dep))
                })
                .unwrap_or(true);

            if !deps_satisfied {
                continue;
            }

            // Execute the skipped step
            let mut skipped_step = match pipeline.get_step(skipped_name) {
                Some(s) => s.clone(),
                None => continue,
            };

            // Inject git credentials if configured
            if skipped_name == "git-pull" {
                if let Some(ref creds) = pipeline.git_credentials {
                    skipped_step
                        .env
                        .insert("GIT_PIPELIGHT_USER".into(), creds.username.clone());
                    skipped_step
                        .env
                        .insert("GIT_PIPELIGHT_PASS".into(), creds.password.clone());
                }
            }

            let sr = if skipped_step.local {
                DockerExecutor::run_step_local(&skipped_step, &project_dir, |_| {}).await?
            } else {
                executor
                    .run_step(&pipeline.name, &skipped_step, &project_dir, |_| {})
                    .await?
            };

            // Update state
            {
                let stdout = sr.stdout_string();
                let stderr = sr.stderr_string();
                let (report_summary, report_path_str) = generate_step_report(
                    &pipeline,
                    &project_dir,
                    &misc_dir,
                    skipped_name,
                    sr.success,
                    &stdout,
                    &stderr,
                );
                let step_test_summary =
                    parse_step_test_summary(&pipeline, skipped_name, &stdout, &stderr);

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
                ss.stdout = if stdout.is_empty() {
                    None
                } else {
                    Some(stdout)
                };
                ss.stderr = if stderr.is_empty() {
                    None
                } else {
                    Some(stderr)
                };
                ss.report_summary = Some(report_summary);
                ss.report_path = Some(report_path_str);
                if step_test_summary.is_some() {
                    ss.test_summary = step_test_summary;
                }

                // Re-resolve on_failure so cascaded steps also surface their
                // callbacks (e.g. `test_print_command` on test failures, or
                // `auto_gen_jacoco_config` when jacoco emits the PIPELIGHT_CALLBACK
                // marker). Without this, LLMs see `on_failure: null` on every step
                // the retry cascade brings up from Skipped, and the callback
                // dispatch rule in `pipelight-run` skill has nothing to act on.
                let sd = step_def_map
                    .as_ref()
                    .and_then(|defs| defs.get(skipped_name));
                let new_on_failure = reresolve_on_failure(
                    sd.map(|b| b.as_ref()),
                    sr.exit_code,
                    ss.stdout.as_deref().unwrap_or(""),
                    ss.stderr.as_deref().unwrap_or(""),
                    None, // cascaded step, no prior retries_remaining to preserve
                    &callback_registry,
                );
                ss.on_failure = new_on_failure;
            }
            let _ = state.save(&base);

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
    let all_success = state
        .steps
        .iter()
        .all(|s| s.status == StepStatus::Success || s.status == StepStatus::Skipped);
    let has_retryable = state.steps.iter().any(|s| {
        s.status == StepStatus::Failed
            && s.on_failure
                .as_ref()
                .map(|of| of.action == CallbackCommandAction::Retry && of.retries_remaining > 0)
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

    // If output_path is relative, resolve it against the target project directory
    let output_path = if output_path.is_relative() {
        dir.join(&output_path)
    } else {
        output_path
    };

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
    println!(
        "Steps: {}",
        pipeline
            .steps
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    for warning in &info.warnings {
        eprintln!("WARNING: {}", warning);
    }

    // Serialize to YAML
    let yaml = serde_yaml::to_string(&pipeline).context("Failed to serialize pipeline to YAML")?;

    // Write file
    std::fs::write(&output_path, &yaml)
        .context(format!("Failed to write {}", output_path.display()))?;

    println!("\nGenerated: {}", output_path.display());
    Ok(0)
}

async fn cmd_list_steps(dir: PathBuf) -> Result<i32> {
    use crate::ci::detector;
    use console::style;

    let (info, pipeline) = detector::detect_and_generate(&dir)?;

    println!(
        "\n{}  {} ({})\n",
        style("Pipeline").cyan().bold(),
        style(&pipeline.name).bold(),
        info.project_type
    );

    for (i, step) in pipeline.steps.iter().enumerate() {
        let deps = if step.depends_on.is_empty() {
            String::new()
        } else {
            format!("  depends on [{}]", step.depends_on.join(", "))
        };

        println!(
            "  {}. {} {}",
            style(i + 1).dim(),
            style(&step.name).bold(),
            style(&deps).dim()
        );
        println!("     image:    {}", style(&step.image).dim());
        for cmd in &step.commands {
            println!("     command:  {}", style(cmd).green());
        }

        if let Some(ref on_failure) = step.on_failure {
            println!(
                "     failure:  {} (max retries: {})",
                style(format!("{:?}", on_failure.callback_command).to_lowercase()).yellow(),
                on_failure.max_retries
            );
        }

        if step.allow_failure {
            println!("     {}", style("allow_failure: true").yellow());
        }

        println!();
    }

    println!("  {} steps total\n", style(pipeline.steps.len()).bold());

    Ok(0)
}

async fn cmd_clean(dir: PathBuf) -> Result<i32> {
    use console::style;

    let dir = dir.canonicalize().unwrap_or(dir);
    let mut removed = Vec::new();

    let pipeline_file = dir.join("pipeline.yml");
    if pipeline_file.exists() {
        std::fs::remove_file(&pipeline_file).context("Failed to remove pipeline.yml")?;
        removed.push("pipeline.yml");
    }

    let misc_dir = dir.join("pipelight-misc");
    if misc_dir.exists() {
        std::fs::remove_dir_all(&misc_dir).context("Failed to remove pipelight-misc/")?;
        removed.push("pipelight-misc/");
    }

    if removed.is_empty() {
        println!("{} Nothing to clean", style("✓").green());
    } else {
        for item in &removed {
            println!("{} Removed {}", style("✓").green(), item);
        }
    }

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

async fn cmd_docker_prepare(file: PathBuf) -> Result<i32> {
    use std::collections::BTreeSet;
    use tracing::info;

    let pipeline = Pipeline::from_file(&file)
        .context(format!("Failed to load pipeline: {}", file.display()))?;

    // Collect unique images (skip local steps and empty images)
    let images: BTreeSet<String> = pipeline
        .steps
        .iter()
        .filter(|s| !s.local && !s.image.is_empty())
        .map(|s| s.image.clone())
        .collect();

    if images.is_empty() {
        println!("No Docker images to pull (all steps are local).");
        return Ok(0);
    }

    println!(
        "Pulling {} Docker image(s) from {}...\n",
        images.len(),
        file.display()
    );

    let executor = DockerExecutor::new().await?;
    let mut failed = Vec::new();

    for image in &images {
        print!("  {} ... ", image);
        match executor.pull_image(image).await {
            Ok(_) => {
                println!("OK");
                info!(image = %image, "Image pulled successfully");
            }
            Err(e) => {
                println!("FAILED ({})", e);
                failed.push(image.clone());
            }
        }
    }

    if !failed.is_empty() {
        println!("\nFailed to pull: {}", failed.join(", "));
        return Ok(1);
    }

    // Install Rust toolchain components (clippy, rustfmt) into images that need them
    let rust_images: BTreeSet<String> = pipeline
        .steps
        .iter()
        .filter(|s| {
            !s.local
                && !s.image.is_empty()
                && s.commands
                    .iter()
                    .any(|c| c.contains("cargo clippy") || c.contains("cargo fmt"))
        })
        .map(|s| s.image.clone())
        .collect();

    for image in &rust_images {
        print!("  Installing clippy+rustfmt in {} ... ", image);
        let setup_result = executor
            .run_setup_container(
                image,
                "rustup component add clippy rustfmt 2>/dev/null || true",
            )
            .await;
        match setup_result {
            Ok(_) => println!("OK"),
            Err(e) => println!("WARN ({})", e),
        }
    }

    println!(
        "\nAll images ready. You can now run: pipelight run -f {}",
        file.display()
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::callback::action::CallbackCommandAction;
    use crate::ci::callback::command::CallbackCommand;
    use crate::ci::pipeline_builder::write_step_report;

    // ── is_cascadable_skipped ────────────────────────────────────

    fn make_step_state(name: &str, status: StepStatus) -> StepState {
        StepState {
            name: name.into(),
            status,
            exit_code: None,
            duration_ms: None,
            image: String::new(),
            command: String::new(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        }
    }

    fn make_pipeline_step(name: &str, active: bool) -> Step {
        Step {
            name: name.into(),
            image: String::new(),
            commands: vec![],
            depends_on: vec![],
            env: Default::default(),
            workdir: "/workspace".into(),
            allow_failure: false,
            condition: None,
            on_failure: None,
            volumes: vec![],
            local: false,
            active,
            tag: String::new(),
        }
    }

    fn make_on_failure_state(
        command: CallbackCommand,
        action: CallbackCommandAction,
        retries_remaining: u32,
    ) -> OnFailureState {
        OnFailureState {
            exception_key: "unrecognized".into(),
            command,
            action,
            max_retries: retries_remaining,
            retries_remaining,
            context_paths: vec![],
        }
    }

    #[test]
    fn test_is_cascadable_skipped_genuine_upstream_hold() {
        // Case 3: step never ran because upstream was failing — should cascade
        let ss = make_step_state("test", StepStatus::Skipped);
        let ps = make_pipeline_step("test", true);
        assert!(is_cascadable_skipped(&ss, Some(&ps)));
    }

    #[test]
    fn test_is_cascadable_skipped_genuine_upstream_hold_without_pipeline_step() {
        // Missing pipeline step is treated as default-active; still cascade
        let ss = make_step_state("test", StepStatus::Skipped);
        assert!(is_cascadable_skipped(&ss, None));
    }

    #[test]
    fn test_is_cascadable_skipped_inactive_step_excluded() {
        // Case 1: ping-pong with active: false — must stay skipped
        let ss = make_step_state("ping-pong", StepStatus::Skipped);
        let ps = make_pipeline_step("ping-pong", false);
        assert!(!is_cascadable_skipped(&ss, Some(&ps)));
    }

    #[test]
    fn test_is_cascadable_skipped_ran_and_skipped_by_policy_excluded() {
        // Case 2: git-pull ran, failed, got skipped by git_fail policy
        let mut ss = make_step_state("git-pull", StepStatus::Skipped);
        ss.exit_code = Some(1);
        ss.duration_ms = Some(5000);
        ss.on_failure = Some(make_on_failure_state(
            CallbackCommand::GitFail,
            CallbackCommandAction::Skip,
            0,
        ));
        let ps = make_pipeline_step("git-pull", true);
        assert!(!is_cascadable_skipped(&ss, Some(&ps)));
    }

    #[test]
    fn test_is_cascadable_skipped_success_excluded() {
        let ss = make_step_state("build", StepStatus::Success);
        let ps = make_pipeline_step("build", true);
        assert!(!is_cascadable_skipped(&ss, Some(&ps)));
    }

    #[test]
    fn test_is_cascadable_skipped_failed_excluded() {
        let ss = make_step_state("pmd", StepStatus::Failed);
        let ps = make_pipeline_step("pmd", true);
        assert!(!is_cascadable_skipped(&ss, Some(&ps)));
    }

    #[test]
    fn test_is_cascadable_skipped_pending_excluded() {
        let ss = make_step_state("test", StepStatus::Pending);
        let ps = make_pipeline_step("test", true);
        assert!(!is_cascadable_skipped(&ss, Some(&ps)));
    }

    #[test]
    fn test_is_cascadable_skipped_running_excluded() {
        let ss = make_step_state("test", StepStatus::Running);
        let ps = make_pipeline_step("test", true);
        assert!(!is_cascadable_skipped(&ss, Some(&ps)));
    }

    #[test]
    fn test_is_cascadable_skipped_inactive_beats_upstream_hold() {
        // Even if pipeline step is inactive AND exit_code is None,
        // inactive flag wins — ping-pong must never be cascaded into
        let ss = make_step_state("ping-pong", StepStatus::Skipped);
        let ps = make_pipeline_step("ping-pong", false);
        assert!(!is_cascadable_skipped(&ss, Some(&ps)));
    }

    #[test]
    fn test_is_cascadable_skipped_exit_code_zero_also_excluded() {
        // Any exit_code (even 0) signals "step ran" — exclude from cascade
        let mut ss = make_step_state("custom", StepStatus::Skipped);
        ss.exit_code = Some(0);
        let ps = make_pipeline_step("custom", true);
        assert!(!is_cascadable_skipped(&ss, Some(&ps)));
    }

    #[test]
    fn test_is_cascadable_skipped_rc_repro_scenario() {
        // Reproduces the rc project state after build succeeded + pmd failed
        // + retry --step pmd cascade should ONLY pick up `test`, not
        // ping-pong (inactive) nor git-pull (skipped by git_fail)
        let ping_pong = make_step_state("ping-pong", StepStatus::Skipped);
        let ping_pong_ps = make_pipeline_step("ping-pong", false);

        let mut git_pull = make_step_state("git-pull", StepStatus::Skipped);
        git_pull.exit_code = Some(1);
        git_pull.on_failure = Some(make_on_failure_state(
            CallbackCommand::GitFail,
            CallbackCommandAction::Skip,
            0,
        ));
        let git_pull_ps = make_pipeline_step("git-pull", true);

        let test_step = make_step_state("test", StepStatus::Skipped);
        let test_ps = make_pipeline_step("test", true);

        assert!(!is_cascadable_skipped(&ping_pong, Some(&ping_pong_ps)));
        assert!(!is_cascadable_skipped(&git_pull, Some(&git_pull_ps)));
        assert!(is_cascadable_skipped(&test_step, Some(&test_ps)));
    }

    // ── write_step_report (existing) ─────────────────────────────

    #[test]
    fn test_write_step_report_both_stdout_and_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_step_report(dir.path(), "build", "compile output", "error: failed");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "compile output\nerror: failed");
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("build-"));
        assert!(path.extension().unwrap() == "log");
    }

    #[test]
    fn test_write_step_report_stdout_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_step_report(dir.path(), "test", "test output here", "");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "test output here");
    }

    #[test]
    fn test_write_step_report_stderr_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_step_report(dir.path(), "package", "", "fatal error");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "fatal error");
    }

    #[test]
    fn test_write_step_report_empty_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_step_report(dir.path(), "lint", "", "");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn test_write_step_report_file_has_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_step_report(dir.path(), "fmt-check", "", "bad format");
        let filename = path.file_name().unwrap().to_string_lossy();
        // Format: fmt-check-20260407T143022.log
        assert!(filename.starts_with("fmt-check-"));
        assert!(filename.ends_with(".log"));
        assert!(filename.len() > "fmt-check-.log".len()); // has timestamp
    }

    // ── generate_step_report / parse_step_test_summary ───────────
    //
    // These helpers are called by both cmd_run and cmd_retry to populate
    // `report_summary`, `report_path` and `test_summary` on a StepState.
    // Before the retry paths used them, `pipelight retry --step pmd` would
    // leave test/package with `report_summary: null` in the JSON output
    // (observed in the rc/wyproject-master retry cascade). These tests lock
    // in that retry now produces identical report fields as initial runs.

    fn make_pipeline(name: &str) -> Pipeline {
        // Include one docker step so `step_defs_for_pipeline` can reconstruct
        // language-specific StepDefs (which carry their own output_report_str).
        // Without this, the cli falls back to BaseStrategy::default_report_str
        // and loses parser-driven summaries.
        let image = if name.starts_with("rust") {
            "rust:latest"
        } else if name.starts_with("maven") {
            "maven:latest"
        } else if name.starts_with("gradle") {
            "gradle:latest"
        } else {
            ""
        };
        let mut steps = vec![];
        if !image.is_empty() {
            let yaml = format!("name: build\nimage: {image}\ncommands: [\"true\"]\n");
            steps.push(serde_yaml::from_str::<crate::ci::parser::Step>(&yaml).unwrap());
        }
        Pipeline {
            name: name.into(),
            git_credentials: None,
            env: Default::default(),
            steps,
        }
    }

    #[test]
    fn test_generate_step_report_maven_package_success() {
        // Maven strategy delegates package summary to BaseStrategy, which
        // returns "Package created" on success regardless of output. Regression
        // guard for the retry cascade bug where package step had empty summary.
        let dir = tempfile::tempdir().unwrap();
        let pipeline = make_pipeline("maven-java-ci");
        let (summary, path) =
            generate_step_report(&pipeline, dir.path(), dir.path(), "package", true, "", "");
        assert_eq!(summary, "Package created");
        // report_path should be relative (strip_prefix against project_dir)
        assert!(path.starts_with("package-"));
        assert!(path.ends_with(".log"));
        // Log file was actually written under misc_dir
        assert!(dir.path().join(&path).exists());
    }

    #[test]
    fn test_generate_step_report_maven_test_with_parsed_summary() {
        // Maven strategy parses `Tests run: N, Failures: X, ...` into a
        // human-readable summary via MavenStrategy::output_report_str.
        let dir = tempfile::tempdir().unwrap();
        let pipeline = make_pipeline("maven-java-ci");
        let stdout = "\
[INFO] Results:
[INFO] Tests run: 42, Failures: 0, Errors: 0, Skipped: 1
[INFO] BUILD SUCCESS
";
        let (summary, _path) =
            generate_step_report(&pipeline, dir.path(), dir.path(), "test", true, stdout, "");
        assert!(
            summary.contains("41 passed"),
            "expected parsed test summary, got: {summary}"
        );
    }

    #[test]
    fn test_generate_step_report_maven_test_falls_back_when_no_surefire_output() {
        // When `mvn test` produces no `Tests run:` line (e.g. no test classes),
        // MavenStrategy falls through to BaseStrategy which returns
        // "Tests passed" / "Tests failed". The prior retry code path left
        // this field as None entirely.
        let dir = tempfile::tempdir().unwrap();
        let pipeline = make_pipeline("maven-java-ci");
        let (summary, _path) = generate_step_report(
            &pipeline,
            dir.path(),
            dir.path(),
            "test",
            true,
            "[INFO] BUILD SUCCESS (no tests)",
            "",
        );
        assert_eq!(summary, "Tests passed");
    }

    #[test]
    fn test_generate_step_report_rust_test_uses_parser() {
        // Regression: previously the cli called PipelineStrategy::output_report_str
        // which dispatched by step name and never reached TestStep::test_parser,
        // so cargo test runs always reported "Tests passed" instead of the parsed
        // "N passed, N failed, N ignored" summary. The fix routes through
        // StepDef::output_report_str on the reconstructed step definitions.
        let dir = tempfile::tempdir().unwrap();
        let pipeline = make_pipeline("rust-ci");
        let stdout = "\
running 5 tests
test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out

running 2 tests
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
";
        let (summary, _path) =
            generate_step_report(&pipeline, dir.path(), dir.path(), "test", true, stdout, "");
        assert_eq!(summary, "5 passed, 0 failed, 1 ignored");
    }

    #[test]
    fn test_generate_step_report_maven_test_failure_markers_surface_build_failure() {
        // When Maven runs with allow_failure=true the executor reports
        // success=true even on `BUILD FAILURE`. The TestStep failure_markers
        // wiring must surface this as "Tests had failures (report-only)" rather
        // than the misleading default "Tests passed".
        let dir = tempfile::tempdir().unwrap();
        let pipeline = make_pipeline("maven-java-ci");
        let stdout = "[INFO] Reactor Summary:\n[INFO] BUILD FAILURE\n";
        let (summary, _path) =
            generate_step_report(&pipeline, dir.path(), dir.path(), "test", true, stdout, "");
        assert_eq!(summary, "Tests had failures (report-only)");
    }

    #[test]
    fn test_generate_step_report_unknown_pipeline_uses_base_strategy() {
        // Pipelines whose name doesn't match any strategy prefix still get a
        // non-empty summary via the BaseStrategy fallback.
        let dir = tempfile::tempdir().unwrap();
        let pipeline = make_pipeline("custom-unknown");
        let (summary, _path) =
            generate_step_report(&pipeline, dir.path(), dir.path(), "package", true, "", "");
        assert_eq!(summary, "Package created");
    }

    #[test]
    fn test_generate_step_report_relative_path_when_misc_outside_project() {
        // When misc_dir is NOT under project_dir (edge case: tests pass the
        // same tempdir for both), the returned path should still be usable.
        let project = tempfile::tempdir().unwrap();
        let misc = tempfile::tempdir().unwrap();
        let pipeline = make_pipeline("maven-java-ci");
        let (_summary, path) = generate_step_report(
            &pipeline,
            project.path(),
            misc.path(),
            "build",
            true,
            "",
            "",
        );
        // strip_prefix fails → returns absolute path from misc_dir
        assert!(std::path::Path::new(&path).is_absolute() || path.contains("build-"));
    }

    #[test]
    fn test_parse_step_test_summary_maven_matches_surefire_format() {
        let pipeline = make_pipeline("maven-java-ci");
        let stdout = "\
[INFO] Tests run: 10, Failures: 1, Errors: 2, Skipped: 3
[INFO] Tests run: 5, Failures: 0, Errors: 0, Skipped: 0
";
        let summary = parse_step_test_summary(&pipeline, "test", stdout, "").unwrap();
        // Totals: run=15, failures=1, errors=2, skipped=3, passed=9
        assert_eq!(summary.passed, 9);
        assert_eq!(summary.failed, 3);
        assert_eq!(summary.skipped, 3);
    }

    #[test]
    fn test_parse_step_test_summary_non_test_step_returns_none() {
        let pipeline = make_pipeline("maven-java-ci");
        assert!(parse_step_test_summary(
            &pipeline,
            "package",
            "Tests run: 10, Failures: 0, Errors: 0, Skipped: 0",
            ""
        )
        .is_none());
    }

    #[test]
    fn test_parse_step_test_summary_unknown_pipeline_returns_none() {
        // Without a strategy there's no parser → None.
        let pipeline = make_pipeline("custom-unknown");
        assert!(parse_step_test_summary(
            &pipeline,
            "test",
            "Tests run: 10, Failures: 0, Errors: 0, Skipped: 0",
            ""
        )
        .is_none());
    }

    #[test]
    fn test_parse_step_test_summary_no_surefire_pattern_returns_none() {
        let pipeline = make_pipeline("maven-java-ci");
        assert!(parse_step_test_summary(&pipeline, "test", "BUILD SUCCESS", "").is_none());
    }

    // ── reresolve_on_failure ─────────────────────────────────────
    //
    // Regression tests for the bug where `cmd_retry` froze `on_failure`
    // to whatever was resolved on the initial failure, so a PMD step that
    // first failed for a missing ruleset kept reporting
    // `auto_gen_pmd_ruleset` even after the ruleset was generated and PMD
    // started reporting real violations.

    fn pmd_step_def() -> Box<dyn crate::ci::pipeline_builder::StepDef> {
        use crate::ci::detector::{ProjectInfo, ProjectType};
        let info = ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: None,
            framework: None,
            image: "maven:3.9".into(),
            build_cmd: vec![],
            test_cmd: vec![],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into(), "src/pom.xml".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        };
        Box::new(crate::ci::pipeline_builder::maven::pmd_step::PmdStep::new(
            &info,
        ))
    }

    #[test]
    fn test_reresolve_switches_ruleset_not_found_to_pmd_violations() {
        let sd = pmd_step_def();
        let registry = CallbackCommandRegistry::new();
        // First-run scenario: ruleset missing -> stderr carries the callback marker.
        let first = reresolve_on_failure(
            Some(sd.as_ref()),
            1,
            "",
            "PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - No pmd-ruleset.xml\n",
            None,
            &registry,
        )
        .expect("first failure should produce on_failure");
        assert_eq!(first.exception_key, "ruleset_not_found");
        assert_eq!(first.command, CallbackCommand::AutoGenPmdRuleset);

        // Second-run scenario (what cmd_retry must emit): ruleset fine, violations found.
        // Must re-resolve to pmd_violations / auto_fix, not stay on the first state.
        let second = reresolve_on_failure(
            Some(sd.as_ref()),
            1,
            "PMD Total: 3 violations\n",
            "",
            Some(first.retries_remaining),
            &registry,
        )
        .expect("second failure should produce on_failure");
        assert_eq!(second.exception_key, "pmd_violations");
        assert_eq!(second.command, CallbackCommand::AutoFix);
        assert_eq!(second.action, CallbackCommandAction::Retry);
        assert!(second
            .context_paths
            .iter()
            .any(|p| p == "pipelight-misc/pmd-report/pmd-result.xml"));
    }

    #[test]
    fn test_reresolve_carries_over_decremented_retries_remaining() {
        let sd = pmd_step_def();
        let registry = CallbackCommandRegistry::new();
        // Caller has already decremented retries_remaining for this attempt.
        let of = reresolve_on_failure(
            Some(sd.as_ref()),
            1,
            "PMD Total: 1 violations\n",
            "",
            Some(3),
            &registry,
        )
        .unwrap();
        assert_eq!(of.max_retries, 9);
        // retries_remaining must reflect the caller's remaining budget, not reset.
        assert_eq!(of.retries_remaining, 3);
    }

    #[test]
    fn test_reresolve_clamps_prev_remaining_to_max_retries() {
        let sd = pmd_step_def();
        let registry = CallbackCommandRegistry::new();
        // If exception changes so the new entry has a smaller max_retries,
        // prev_remaining must be clamped down to the new max.
        // (ruleset_invalid and pmd_violations currently share max_retries=9,
        //  so we simulate the bound with an oversized prev value.)
        let of = reresolve_on_failure(
            Some(sd.as_ref()),
            1,
            "PMD Total: 1 violations\n",
            "",
            Some(999),
            &registry,
        )
        .unwrap();
        assert!(of.retries_remaining <= of.max_retries);
        assert_eq!(of.retries_remaining, of.max_retries);
    }

    #[test]
    fn test_reresolve_returns_none_when_no_step_def() {
        let registry = CallbackCommandRegistry::new();
        let of = reresolve_on_failure(None, 1, "PMD Total: 3 violations\n", "", Some(5), &registry);
        assert!(of.is_none());
    }

    #[test]
    fn test_reresolve_returns_none_on_success_without_match() {
        let sd = pmd_step_def();
        let registry = CallbackCommandRegistry::new();
        // Success (exit=0) with stdout/stderr that don't match any rule -> no callback.
        let of = reresolve_on_failure(
            Some(sd.as_ref()),
            0,
            "PMD: no changed source files on current branch — skipping\n",
            "",
            None,
            &registry,
        );
        assert!(of.is_none());
    }

    // ── cascade_dep_satisfied ────────────────────────────────────

    fn make_pipeline_with_dep_chain() -> Pipeline {
        // test → pmd_full (inactive/skipped) → pmd
        let mut pipeline = make_pipeline("maven-java-ci");
        pipeline.steps = vec![
            {
                let mut s = make_pipeline_step("pmd", true);
                s.image = "maven:latest".into();
                s
            },
            {
                let mut s = make_pipeline_step("pmd_full", false);
                s.depends_on = vec!["pmd".into()];
                s.image = "maven:latest".into();
                s
            },
            {
                let mut s = make_pipeline_step("test", true);
                s.depends_on = vec!["pmd_full".into()];
                s.image = "maven:latest".into();
                s
            },
        ];
        pipeline
    }

    fn make_state_with(steps: Vec<StepState>) -> RunState {
        let mut state = RunState::new("run-test", "maven-java-ci");
        for s in steps {
            state.add_step(s);
        }
        state
    }

    #[test]
    fn test_cascade_dep_satisfied_success_dep() {
        let pipeline = make_pipeline_with_dep_chain();
        let state = make_state_with(vec![make_step_state("pmd_full", StepStatus::Success)]);
        assert!(cascade_dep_satisfied(&state, &pipeline, "pmd_full"));
    }

    #[test]
    fn test_cascade_dep_satisfied_inactive_skipped_dep() {
        // Regression: test.depends_on = [pmd_full]; pmd_full is inactive and
        // therefore Skipped. Must NOT block the cascade.
        let pipeline = make_pipeline_with_dep_chain();
        let state = make_state_with(vec![make_step_state("pmd_full", StepStatus::Skipped)]);
        assert!(cascade_dep_satisfied(&state, &pipeline, "pmd_full"));
    }

    #[test]
    fn test_cascade_dep_satisfied_active_skipped_dep_blocks() {
        // An ACTIVE step that was skipped (e.g. upstream-hold) is not a pass —
        // downstream must wait until it actually succeeds.
        let pipeline = make_pipeline_with_dep_chain();
        let state = make_state_with(vec![make_step_state("pmd", StepStatus::Skipped)]);
        assert!(!cascade_dep_satisfied(&state, &pipeline, "pmd"));
    }

    #[test]
    fn test_cascade_dep_satisfied_failed_dep_blocks() {
        let pipeline = make_pipeline_with_dep_chain();
        let state = make_state_with(vec![make_step_state("pmd_full", StepStatus::Failed)]);
        assert!(!cascade_dep_satisfied(&state, &pipeline, "pmd_full"));
    }

    #[test]
    fn test_cascade_dep_satisfied_missing_dep() {
        let pipeline = make_pipeline_with_dep_chain();
        let state = make_state_with(vec![]);
        assert!(!cascade_dep_satisfied(&state, &pipeline, "pmd_full"));
    }

    // ── incremental state.save() ──────────────────────────────────
    //
    // Verifies that status.json is written after each step completes,
    // not only at the end of the pipeline. External tools (e.g. the
    // pipelight-run skill) can monitor this file to track progress
    // without waiting for the full pipeline to finish.

    #[test]
    fn test_incremental_save_after_add_step() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let mut state = RunState::new("incr-1", "test-pipeline");

        // Simulate first step completing and saving
        state.add_step(make_step_state("build", StepStatus::Success));
        state.save(base).unwrap();

        let loaded = RunState::load(base, "incr-1").unwrap();
        assert_eq!(loaded.steps.len(), 1);
        assert_eq!(loaded.steps[0].name, "build");
        assert_eq!(loaded.steps[0].status, StepStatus::Success);
        assert_eq!(loaded.status, PipelineStatus::Running);

        // Simulate second step completing and saving
        state.add_step(make_step_state("test", StepStatus::Failed));
        state.save(base).unwrap();

        let loaded = RunState::load(base, "incr-1").unwrap();
        assert_eq!(loaded.steps.len(), 2);
        assert_eq!(loaded.steps[1].name, "test");
        assert_eq!(loaded.steps[1].status, StepStatus::Failed);
        // Pipeline status still Running (not finalized yet)
        assert_eq!(loaded.status, PipelineStatus::Running);
    }

    #[test]
    fn test_incremental_save_skipped_step_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let mut state = RunState::new("incr-2", "test-pipeline");

        state.add_step(make_step_state("ping-pong", StepStatus::Skipped));
        state.save(base).unwrap();

        let loaded = RunState::load(base, "incr-2").unwrap();
        assert_eq!(loaded.steps.len(), 1);
        assert_eq!(loaded.steps[0].status, StepStatus::Skipped);
    }

    #[test]
    fn test_incremental_save_overwrites_previous() {
        // Each save overwrites the file — the last save contains all steps
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let mut state = RunState::new("incr-3", "test-pipeline");

        state.add_step(make_step_state("build", StepStatus::Success));
        state.save(base).unwrap();

        state.add_step(make_step_state("test", StepStatus::Success));
        state.save(base).unwrap();

        state.add_step(make_step_state("lint", StepStatus::Success));
        state.save(base).unwrap();

        // Only one file, contains all 3 steps
        let loaded = RunState::load(base, "incr-3").unwrap();
        assert_eq!(loaded.steps.len(), 3);
        let names: Vec<&str> = loaded.steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "test", "lint"]);
    }

    #[test]
    fn test_incremental_save_retry_updates_existing_step() {
        // Simulates what cmd_retry does: mutate an existing step then save
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let mut state = RunState::new("incr-4", "test-pipeline");

        state.add_step(make_step_state("build", StepStatus::Failed));
        state.save(base).unwrap();

        // Retry: update the failed step to success
        let ss = state.get_step_mut("build").unwrap();
        ss.status = StepStatus::Success;
        ss.exit_code = Some(0);
        ss.duration_ms = Some(1234);
        state.save(base).unwrap();

        let loaded = RunState::load(base, "incr-4").unwrap();
        assert_eq!(loaded.steps.len(), 1);
        assert_eq!(loaded.steps[0].status, StepStatus::Success);
        assert_eq!(loaded.steps[0].exit_code, Some(0));
        assert_eq!(loaded.steps[0].duration_ms, Some(1234));
    }

    #[test]
    fn test_incremental_save_file_readable_as_valid_json() {
        // Ensures the file is always valid JSON (not corrupted by partial writes)
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let mut state = RunState::new("incr-5", "test-pipeline");

        for i in 0..5 {
            state.add_step(make_step_state(&format!("step-{i}"), StepStatus::Success));
            state.save(base).unwrap();

            // Read raw file and verify it's valid JSON
            let path = base.join("incr-5").join("status.json");
            let content = std::fs::read_to_string(&path).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content)
                .expect("status.json must be valid JSON after each incremental save");
            let steps = parsed["steps"].as_array().unwrap();
            assert_eq!(steps.len(), i + 1);
        }
    }

    #[test]
    fn test_reresolve_prefers_stderr_markers_over_stdout() {
        let sd = pmd_step_def();
        let registry = CallbackCommandRegistry::new();
        // Even if stdout contains "PMD Total:", stderr errors take precedence
        // (ruleset load failures win over violation counts).
        let of = reresolve_on_failure(
            Some(sd.as_ref()),
            1,
            "PMD Total: 0 violations\n",
            "Cannot load ruleset pipelight-misc/pmd-ruleset.xml\n",
            None,
            &registry,
        )
        .unwrap();
        assert_eq!(of.exception_key, "ruleset_invalid");
        assert_eq!(of.command, CallbackCommand::AutoGenPmdRuleset);
    }

    // ── --git-diff-from-remote-branch flag ───────────────────────

    #[test]
    fn test_cli_parses_git_diff_from_remote_branch_flag() {
        use clap::Parser;
        let cli = Cli::try_parse_from([
            "pipelight",
            "run",
            "--git-diff-from-remote-branch=origin/main",
        ])
        .expect("should parse");
        match cli.command {
            Some(Command::Run {
                git_diff_from_remote_branch,
                ..
            }) => assert_eq!(git_diff_from_remote_branch.as_deref(), Some("origin/main")),
            _ => panic!("expected Run subcommand"),
        }
    }

    #[test]
    fn test_cli_git_diff_flag_defaults_to_none() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["pipelight", "run"]).expect("should parse");
        match cli.command {
            Some(Command::Run {
                git_diff_from_remote_branch,
                ..
            }) => assert_eq!(git_diff_from_remote_branch, None),
            _ => panic!("expected Run subcommand"),
        }
    }

    #[test]
    fn test_is_safe_ref_accepts_typical_remote_branches() {
        assert!(is_safe_ref("origin/main"));
        assert!(is_safe_ref("origin/release/v1.2.3"));
        assert!(is_safe_ref("upstream/feature_branch-42"));
    }

    #[test]
    fn test_is_safe_ref_rejects_shell_metachars() {
        assert!(!is_safe_ref("origin/main;rm -rf /"));
        assert!(!is_safe_ref("origin/`whoami`"));
        assert!(!is_safe_ref("origin/$(pwd)"));
        assert!(!is_safe_ref("\"origin/main\""));
        assert!(!is_safe_ref("origin/main\\"));
        assert!(!is_safe_ref("")); // empty rejected
    }

    #[test]
    fn test_cli_retry_parses_git_diff_flag() {
        use clap::Parser;
        let cli = Cli::try_parse_from([
            "pipelight",
            "retry",
            "--run-id=abc",
            "--step=git-diff",
            "--git-diff-from-remote-branch=origin/develop",
        ])
        .expect("should parse");
        match cli.command {
            Some(Command::Retry {
                git_diff_from_remote_branch,
                ..
            }) => assert_eq!(
                git_diff_from_remote_branch.as_deref(),
                Some("origin/develop")
            ),
            _ => panic!("expected Retry subcommand"),
        }
    }

    #[test]
    fn test_cli_retry_without_git_diff_flag() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["pipelight", "retry", "--run-id=abc", "--step=pmd"])
            .expect("should parse");
        match cli.command {
            Some(Command::Retry {
                git_diff_from_remote_branch,
                ..
            }) => assert_eq!(git_diff_from_remote_branch, None),
            _ => panic!("expected Retry subcommand"),
        }
    }
}
