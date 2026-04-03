# AI-Native CI/CD Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the run-exit-fix-retry loop with structured JSON output, on_failure YAML extension, and status.json state persistence.

**Architecture:** Extend existing 5-layer architecture. Add `OutputMode` enum to switch between TTY/Plain/JSON. Add `on_failure` to pipeline model. Add `RunState` for status.json persistence. Add `retry` and `status` CLI subcommands.

**Tech Stack:** Rust, serde/serde_json/serde_yaml, clap, tokio, bollard, uuid

---

## File Structure

```
src/
  main.rs                  → modify: add process exit codes
  cli/mod.rs               → modify: add --output, --run-id flags, retry/status subcommands
  pipeline/mod.rs           → modify: add OnFailure, Strategy to Step model
  scheduler/mod.rs          → no changes
  executor/mod.rs           → modify: capture stdout/stderr as strings
  output/mod.rs             → modify: refactor into OutputMode dispatch
  output/tty.rs             → create: existing TTY output logic extracted here
  output/json.rs            → create: JSON output formatter
  output/plain.rs           → create: plain text output formatter
  run_state/mod.rs          → create: RunState model + status.json read/write
Cargo.toml                 → modify: add serde_json dependency
```

---

### Task 1: Add serde_json dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add serde_json to Cargo.toml**

Add under `[dependencies]`:

```toml
serde_json = "1"
```

- [ ] **Step 2: Verify build**

Run: `cargo build`
Expected: compiles with no new errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add serde_json dependency"
```

---

### Task 2: Add on_failure to pipeline model

**Files:**
- Modify: `src/pipeline/mod.rs`
- Test: `src/pipeline/mod.rs` (inline tests)

- [ ] **Step 1: Write failing test for on_failure parsing**

Add to the `#[cfg(test)] mod tests` block in `src/pipeline/mod.rs`:

```rust
#[test]
fn test_parse_on_failure() {
    let yaml = r#"
name: test-pipeline
steps:
  - name: build
    image: rust:1.78
    commands:
      - cargo build
    on_failure:
      strategy: auto_fix
      max_retries: 3
      context_paths:
        - src/
        - Cargo.toml
"#;
    let pipeline = Pipeline::from_str(yaml).unwrap();
    let step = &pipeline.steps[0];
    let on_failure = step.on_failure.as_ref().unwrap();
    assert_eq!(on_failure.strategy, Strategy::AutoFix);
    assert_eq!(on_failure.max_retries, 3);
    assert_eq!(on_failure.context_paths, vec!["src/", "Cargo.toml"]);
}

#[test]
fn test_on_failure_defaults() {
    let yaml = r#"
name: test-pipeline
steps:
  - name: build
    image: rust:1.78
    commands:
      - cargo build
"#;
    let pipeline = Pipeline::from_str(yaml).unwrap();
    let step = &pipeline.steps[0];
    assert!(step.on_failure.is_none());
}

#[test]
fn test_on_failure_notify_strategy() {
    let yaml = r#"
name: test-pipeline
steps:
  - name: test
    image: rust:1.78
    commands:
      - cargo test
    on_failure:
      strategy: notify
"#;
    let pipeline = Pipeline::from_str(yaml).unwrap();
    let step = &pipeline.steps[0];
    let on_failure = step.on_failure.as_ref().unwrap();
    assert_eq!(on_failure.strategy, Strategy::Notify);
    assert_eq!(on_failure.max_retries, 0);
    assert!(on_failure.context_paths.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib pipeline`
Expected: FAIL — `Strategy` and `OnFailure` types do not exist yet

- [ ] **Step 3: Implement OnFailure and Strategy types**

Add these structs above the `impl Pipeline` block in `src/pipeline/mod.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Abort,
    AutoFix,
    Notify,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnFailure {
    #[serde(default = "default_strategy")]
    pub strategy: Strategy,

    #[serde(default)]
    pub max_retries: u32,

    #[serde(default)]
    pub context_paths: Vec<String>,
}

fn default_strategy() -> Strategy {
    Strategy::Abort
}
```

Add the `on_failure` field to the `Step` struct:

```rust
/// Failure handling strategy (optional, defaults to abort)
#[serde(default)]
pub on_failure: Option<OnFailure>,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib pipeline`
Expected: all pipeline tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/mod.rs
git commit -m "feat: add on_failure and Strategy to pipeline model"
```

---

### Task 3: Add OutputMode enum and --output flag

**Files:**
- Modify: `src/cli/mod.rs`
- Create: `src/output/tty.rs`
- Create: `src/output/json.rs`
- Create: `src/output/plain.rs`
- Modify: `src/output/mod.rs`

- [ ] **Step 1: Define OutputMode enum**

Add to the top of `src/output/mod.rs`, replacing existing content:

```rust
pub mod tty;
pub mod json;
pub mod plain;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum OutputMode {
    Tty,
    Plain,
    Json,
}

impl OutputMode {
    /// Auto-detect output mode based on stdout
    pub fn detect() -> Self {
        if atty::is(atty::Stream::Stdout) {
            OutputMode::Tty
        } else {
            OutputMode::Plain
        }
    }

    /// Parse from CLI flag string
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "tty" => Ok(OutputMode::Tty),
            "plain" => Ok(OutputMode::Plain),
            "json" => Ok(OutputMode::Json),
            _ => Err(format!("Invalid output mode: '{}'. Must be tty, plain, or json", s)),
        }
    }
}
```

- [ ] **Step 2: Move existing TTY output logic to tty.rs**

Create `src/output/tty.rs` with the current `PipelineReporter` code (the entire current content of output/mod.rs except the new OutputMode). Update imports to reference types from parent module or crate root.

- [ ] **Step 3: Create placeholder json.rs**

Create `src/output/json.rs`:

```rust
use serde::Serialize;
use crate::run_state::RunState;

/// Output a RunState as JSON to stdout
pub fn print_run_state(state: &RunState) {
    let json = serde_json::to_string_pretty(state).expect("Failed to serialize RunState");
    println!("{}", json);
}
```

Note: This will not compile yet — `RunState` is created in Task 5. This is a placeholder.

- [ ] **Step 4: Create placeholder plain.rs**

Create `src/output/plain.rs`:

```rust
use crate::run_state::RunState;

/// Output a RunState as plain text to stdout
pub fn print_run_state(state: &RunState) {
    println!("Pipeline: {} [{}]", state.pipeline, state.status);
    for step in &state.steps {
        println!("  {} — {}", step.name, step.status);
        if let Some(ref stderr) = step.stderr {
            if !stderr.is_empty() {
                for line in stderr.lines().take(10) {
                    println!("    | {}", line);
                }
            }
        }
    }
}
```

- [ ] **Step 5: Add --output flag to CLI**

Modify `src/cli/mod.rs` — add `--output` to `Run` command:

```rust
/// Output format: tty, plain, json (default: auto-detect)
#[arg(long, default_value = None)]
output: Option<String>,
```

- [ ] **Step 6: Add atty dependency to Cargo.toml**

```toml
atty = "0.2"
```

- [ ] **Step 7: Verify build (expect some errors from placeholders — that's OK)**

Run: `cargo build 2>&1`
Note: json.rs and plain.rs reference `RunState` which doesn't exist yet. Comment them out temporarily or use `#[allow(dead_code)]` and placeholder types. The important thing is the OutputMode enum and --output flag compile.

- [ ] **Step 8: Commit**

```bash
git add src/output/ src/cli/mod.rs Cargo.toml Cargo.lock
git commit -m "feat: add OutputMode enum, --output flag, split output into tty/json/plain modules"
```

---

### Task 4: Add --run-id flag and retry/status subcommands

**Files:**
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Add --run-id to Run command**

```rust
/// Run ID for state persistence (auto-generated if omitted)
#[arg(long)]
run_id: Option<String>,
```

- [ ] **Step 2: Add Retry subcommand**

```rust
/// Retry a failed step from a previous run
Retry {
    /// Run ID of the previous run
    #[arg(long)]
    run_id: String,

    /// Step name to retry
    #[arg(long)]
    step: String,

    /// Output format: tty, plain, json
    #[arg(long)]
    output: Option<String>,

    /// Path to pipeline config file
    #[arg(short, long, default_value = "pipeline.yml")]
    file: PathBuf,
},
```

- [ ] **Step 3: Add Status subcommand**

```rust
/// Check status of a pipeline run
Status {
    /// Run ID to check
    #[arg(long)]
    run_id: String,

    /// Output format: tty, plain, json
    #[arg(long)]
    output: Option<String>,
},
```

- [ ] **Step 4: Add placeholder dispatch arms**

In `dispatch()` function, add placeholder match arms that return `Ok(())` for now:

```rust
Command::Retry { run_id, step, output, file } => {
    tracing::info!(run_id = %run_id, step = %step, "Retry not yet implemented");
    Ok(())
}
Command::Status { run_id, output } => {
    tracing::info!(run_id = %run_id, "Status not yet implemented");
    Ok(())
}
```

- [ ] **Step 5: Verify build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 6: Verify CLI help shows new subcommands**

Run: `cargo run -- --help`
Expected: shows `run`, `validate`, `list`, `retry`, `status`

Run: `cargo run -- retry --help`
Expected: shows `--run-id`, `--step`, `--output`, `--file` flags

- [ ] **Step 7: Commit**

```bash
git add src/cli/mod.rs
git commit -m "feat: add --run-id flag, retry and status subcommands (placeholder)"
```

---

### Task 5: Implement RunState model and status.json persistence

**Files:**
- Create: `src/run_state/mod.rs`
- Modify: `src/main.rs`
- Test: `src/run_state/mod.rs` (inline tests)

- [ ] **Step 1: Write failing tests**

Create `src/run_state/mod.rs` with test block:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_create_and_save_run_state() {
        let dir = tempfile::tempdir().unwrap();
        let state = RunState::new("test-run-123", "my-pipeline");
        state.save(dir.path()).unwrap();

        let loaded = RunState::load(dir.path(), "test-run-123").unwrap();
        assert_eq!(loaded.run_id, "test-run-123");
        assert_eq!(loaded.pipeline, "my-pipeline");
        assert_eq!(loaded.status, PipelineStatus::Running);
        assert!(loaded.steps.is_empty());
    }

    #[test]
    fn test_update_step_status() {
        let mut state = RunState::new("run-1", "pipeline-1");
        state.add_step("build", StepState {
            name: "build".into(),
            status: StepStatus::Running,
            exit_code: None,
            duration_ms: None,
            image: "rust:1.78".into(),
            command: "cargo build".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
        });

        state.update_step("build", StepStatus::Failed, Some(101), Some(8200));
        let step = state.get_step("build").unwrap();
        assert_eq!(step.status, StepStatus::Failed);
        assert_eq!(step.exit_code, Some(101));
    }

    #[test]
    fn test_retries_remaining() {
        let mut state = RunState::new("run-1", "pipeline-1");
        state.add_step("build", StepState {
            name: "build".into(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            duration_ms: Some(1000),
            image: "rust:1.78".into(),
            command: "cargo build".into(),
            stdout: None,
            stderr: Some("error".into()),
            error_context: None,
            on_failure: Some(OnFailureState {
                strategy: "auto_fix".into(),
                max_retries: 3,
                retries_remaining: 3,
                context_paths: vec!["src/".into()],
            }),
        });

        state.decrement_retries("build");
        let step = state.get_step("build").unwrap();
        assert_eq!(step.on_failure.as_ref().unwrap().retries_remaining, 2);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Add `mod run_state;` to `src/main.rs` first.

Run: `cargo test --lib run_state`
Expected: FAIL — types don't exist yet

- [ ] **Step 3: Implement RunState model**

Add to `src/run_state/mod.rs` above the test block:

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Running,
    Success,
    Retryable,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    pub files: Vec<String>,
    pub lines: Vec<u32>,
    pub error_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnFailureState {
    pub strategy: String,
    pub max_retries: u32,
    pub retries_remaining: u32,
    pub context_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepState {
    pub name: String,
    pub status: StepStatus,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<u64>,
    pub image: String,
    pub command: String,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub error_context: Option<ErrorContext>,
    pub on_failure: Option<OnFailureState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    pub pipeline: String,
    pub status: PipelineStatus,
    pub duration_ms: Option<u64>,
    pub steps: Vec<StepState>,
}

impl RunState {
    pub fn new(run_id: &str, pipeline_name: &str) -> Self {
        Self {
            run_id: run_id.to_string(),
            pipeline: pipeline_name.to_string(),
            status: PipelineStatus::Running,
            duration_ms: None,
            steps: Vec::new(),
        }
    }

    pub fn add_step(&mut self, _name: &str, step: StepState) {
        self.steps.push(step);
    }

    pub fn get_step(&self, name: &str) -> Option<&StepState> {
        self.steps.iter().find(|s| s.name == name)
    }

    pub fn get_step_mut(&mut self, name: &str) -> Option<&mut StepState> {
        self.steps.iter_mut().find(|s| s.name == name)
    }

    pub fn update_step(
        &mut self,
        name: &str,
        status: StepStatus,
        exit_code: Option<i64>,
        duration_ms: Option<u64>,
    ) {
        if let Some(step) = self.get_step_mut(name) {
            step.status = status;
            step.exit_code = exit_code;
            step.duration_ms = duration_ms;
        }
    }

    pub fn decrement_retries(&mut self, name: &str) {
        if let Some(step) = self.get_step_mut(name) {
            if let Some(ref mut on_failure) = step.on_failure {
                if on_failure.retries_remaining > 0 {
                    on_failure.retries_remaining -= 1;
                }
            }
        }
    }

    /// Get the directory path for this run
    fn run_dir(base: &Path, run_id: &str) -> PathBuf {
        base.join(run_id)
    }

    /// Save state to status.json
    pub fn save(&self, base: &Path) -> Result<()> {
        let dir = Self::run_dir(base, &self.run_id);
        std::fs::create_dir_all(&dir)
            .context(format!("Failed to create run directory: {}", dir.display()))?;
        let path = dir.join("status.json");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)
            .context(format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    /// Load state from status.json
    pub fn load(base: &Path, run_id: &str) -> Result<Self> {
        let path = Self::run_dir(base, run_id).join("status.json");
        let content = std::fs::read_to_string(&path)
            .context(format!("Failed to read {}", path.display()))?;
        let state: Self = serde_json::from_str(&content)
            .context("Failed to parse status.json")?;
        Ok(state)
    }

    /// Default base directory for run state
    pub fn default_base_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".pipelight")
            .join("runs")
    }
}
```

- [ ] **Step 4: Add tempfile and dirs dependencies**

Add to `Cargo.toml`:

```toml
dirs = "5"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 5: Add `mod run_state;` to main.rs**

Add `mod run_state;` to `src/main.rs`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib run_state`
Expected: all 3 tests PASS

- [ ] **Step 7: Commit**

```bash
git add src/run_state/ src/main.rs Cargo.toml Cargo.lock
git commit -m "feat: implement RunState model with status.json persistence"
```

---

### Task 6: Wire up JSON and Plain output modules

**Files:**
- Modify: `src/output/json.rs`
- Modify: `src/output/plain.rs`
- Modify: `src/output/mod.rs`

- [ ] **Step 1: Implement json.rs with RunState**

Update `src/output/json.rs`:

```rust
use crate::run_state::RunState;

pub fn print_run_state(state: &RunState) {
    let json = serde_json::to_string_pretty(state).expect("Failed to serialize RunState");
    println!("{}", json);
}
```

- [ ] **Step 2: Implement plain.rs with RunState**

Update `src/output/plain.rs`:

```rust
use crate::run_state::{RunState, StepStatus};

pub fn print_run_state(state: &RunState) {
    println!("Pipeline: {} [{}]", state.pipeline, format!("{:?}", state.status).to_lowercase());
    if let Some(ms) = state.duration_ms {
        println!("Duration: {:.1}s", ms as f64 / 1000.0);
    }
    println!();
    for step in &state.steps {
        let icon = match step.status {
            StepStatus::Success => "[OK]",
            StepStatus::Failed => "[FAIL]",
            StepStatus::Skipped => "[SKIP]",
            StepStatus::Running => "[..]",
            StepStatus::Pending => "[--]",
        };
        println!("  {} {} ({})", icon, step.name, step.image);
        if step.status == StepStatus::Failed {
            if let Some(ref stderr) = step.stderr {
                for line in stderr.lines().take(10) {
                    println!("    | {}", line);
                }
            }
        }
    }
}
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/output/
git commit -m "feat: implement json and plain output formatters"
```

---

### Task 7: Implement the run command with RunState integration

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/executor/mod.rs`
- Modify: `src/main.rs`

This is the biggest task — wiring `pipelight run` to create RunState, execute steps, populate state, handle on_failure strategy, save status.json, output in the correct format, and exit with the correct code.

- [ ] **Step 1: Modify executor to return stdout/stderr as strings**

In `src/executor/mod.rs`, the current `StepResult` has `logs: Vec<LogLine>`. Add convenience methods to extract stdout and stderr as single strings:

```rust
impl StepResult {
    pub fn stdout_string(&self) -> String {
        self.logs.iter()
            .filter(|l| l.stream == LogStream::Stdout)
            .map(|l| l.message.as_str())
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn stderr_string(&self) -> String {
        self.logs.iter()
            .filter(|l| l.stream == LogStream::Stderr)
            .map(|l| l.message.as_str())
            .collect::<Vec<_>>()
            .join("")
    }
}
```

- [ ] **Step 2: Rewrite cmd_run to use RunState**

Rewrite `cmd_run` in `src/cli/mod.rs` to:
1. Generate run_id (from flag or UUID)
2. Create RunState
3. Execute steps via scheduler
4. For each step result, populate StepState with on_failure from pipeline config
5. On failure with `auto_fix` strategy: set pipeline status to `Retryable`, skip downstream, save and exit
6. On failure with `abort`/`notify`/no on_failure: set pipeline status to `Failed`, skip downstream, save and exit
7. On all success: set pipeline status to `Success`
8. Save status.json
9. Output based on OutputMode

```rust
async fn cmd_run(
    file: PathBuf,
    step_filter: Option<String>,
    dry_run: bool,
    output_mode: OutputMode,
    run_id: Option<String>,
) -> Result<i32> {
    let pipeline = Pipeline::from_file(&file)
        .context(format!("Failed to load pipeline: {}", file.display()))?;
    let scheduler = Scheduler::new(&pipeline)?;

    if dry_run {
        let reporter = tty::PipelineReporter::new();
        reporter.print_execution_plan(&pipeline, &scheduler);
        return Ok(0);
    }

    let run_id = run_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());
    let base_dir = RunState::default_base_dir();
    let mut state = RunState::new(&run_id, &pipeline.name);

    let executor = DockerExecutor::new().await?;
    let schedule = scheduler.resolve(step_filter.as_deref())?;
    let start = std::time::Instant::now();

    if output_mode == OutputMode::Tty {
        let reporter = tty::PipelineReporter::new();
        reporter.print_header(&pipeline);
    }

    let mut has_retryable_failure = false;
    let mut has_final_failure = false;

    for batch in &schedule {
        let handles: Vec<_> = batch
            .iter()
            .map(|step_name| {
                let executor = executor.clone();
                let step = pipeline.get_step(step_name).expect("step must exist").clone();
                let pipeline_name = pipeline.name.clone();
                tokio::spawn(async move { executor.run_step(&pipeline_name, &step).await })
            })
            .collect();

        for (i, handle) in handles.into_iter().enumerate() {
            let step_name = &batch[i];
            let step_config = pipeline.get_step(step_name).unwrap();
            let result = handle.await??;

            if output_mode == OutputMode::Tty {
                let reporter = tty::PipelineReporter::new();
                reporter.print_step_result(&result);
            }

            // Build on_failure state from config
            let on_failure_state = step_config.on_failure.as_ref().map(|of| {
                OnFailureState {
                    strategy: format!("{:?}", of.strategy).to_lowercase(),
                    max_retries: of.max_retries,
                    retries_remaining: of.max_retries,
                    context_paths: of.context_paths.clone(),
                }
            });

            let step_status = if result.success {
                StepStatus::Success
            } else {
                StepStatus::Failed
            };

            state.add_step(step_name, StepState {
                name: step_name.clone(),
                status: step_status.clone(),
                exit_code: Some(result.exit_code),
                duration_ms: Some(result.duration.as_millis() as u64),
                image: step_config.image.clone(),
                command: step_config.commands.join(" && "),
                stdout: Some(result.stdout_string()),
                stderr: Some(result.stderr_string()),
                error_context: None, // best-effort parsing, deferred
                on_failure: on_failure_state,
            });

            if !result.success && !step_config.allow_failure {
                let strategy = step_config.on_failure.as_ref()
                    .map(|of| &of.strategy)
                    .unwrap_or(&Strategy::Abort);

                match strategy {
                    Strategy::AutoFix => {
                        has_retryable_failure = true;
                        // Mark remaining steps as skipped and break
                    }
                    Strategy::Abort | Strategy::Notify => {
                        has_final_failure = true;
                    }
                }
                // Skip remaining batches
                break;
            }
        }

        if has_retryable_failure || has_final_failure {
            break;
        }
    }

    state.duration_ms = Some(start.elapsed().as_millis() as u64);

    if has_retryable_failure {
        state.status = PipelineStatus::Retryable;
    } else if has_final_failure {
        state.status = PipelineStatus::Failed;
    } else {
        state.status = PipelineStatus::Success;
    }

    state.save(&base_dir)?;

    match output_mode {
        OutputMode::Json => json::print_run_state(&state),
        OutputMode::Plain => plain::print_run_state(&state),
        OutputMode::Tty => {
            let reporter = tty::PipelineReporter::new();
            reporter.print_summary();
        }
    }

    // Return exit code
    if has_retryable_failure {
        Ok(1)
    } else if has_final_failure {
        Ok(2)
    } else {
        Ok(0)
    }
}
```

- [ ] **Step 3: Update main.rs to use process exit codes**

Modify `src/main.rs`:

```rust
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
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
```

- [ ] **Step 4: Update dispatch to return exit codes**

Change `dispatch` return type to `Result<i32>`. Commands that don't need exit codes return `Ok(0)`.

- [ ] **Step 5: Verify build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 6: Test manually with pipeline.yml**

Run: `cargo run -- run -f pipeline.yml --output json`
Expected: JSON output with run_id, steps, status. (Requires Docker running)

Run: `ls ~/.pipelight/runs/`
Expected: directory with run-id containing status.json

- [ ] **Step 7: Commit**

```bash
git add src/
git commit -m "feat: wire run command with RunState, output modes, and exit codes"
```

---

### Task 8: Implement retry command

**Files:**
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Implement cmd_retry**

```rust
async fn cmd_retry(
    run_id: String,
    step_name: String,
    output_mode: OutputMode,
    file: PathBuf,
) -> Result<i32> {
    let base_dir = RunState::default_base_dir();
    let mut state = RunState::load(&base_dir, &run_id)
        .context(format!("No run found with id '{}'", run_id))?;

    // Check the step exists and is failed
    let step_state = state.get_step(&step_name)
        .ok_or_else(|| anyhow::anyhow!("Step '{}' not found in run '{}'", step_name, run_id))?;

    if step_state.status != StepStatus::Failed {
        anyhow::bail!("Step '{}' is not in failed state (current: {:?})", step_name, step_state.status);
    }

    // Check retries remaining
    if let Some(ref on_failure) = step_state.on_failure {
        if on_failure.retries_remaining == 0 {
            anyhow::bail!("Step '{}' has exhausted all retries", step_name);
        }
    }

    // Decrement retries
    state.decrement_retries(&step_name);

    // Load pipeline to get step config
    let pipeline = Pipeline::from_file(&file)
        .context(format!("Failed to load pipeline: {}", file.display()))?;
    let scheduler = Scheduler::new(&pipeline)?;

    // Re-execute the failed step
    let executor = DockerExecutor::new().await?;
    let step_config = pipeline.get_step(&step_name)
        .ok_or_else(|| anyhow::anyhow!("Step '{}' not found in pipeline", step_name))?;

    let start = std::time::Instant::now();
    let result = executor.run_step(&pipeline.name, step_config).await?;

    // Update step state
    let new_status = if result.success { StepStatus::Success } else { StepStatus::Failed };
    state.update_step(
        &step_name,
        new_status.clone(),
        Some(result.exit_code),
        Some(result.duration.as_millis() as u64),
    );

    // Update stdout/stderr
    if let Some(step) = state.get_step_mut(&step_name) {
        step.stdout = Some(result.stdout_string());
        step.stderr = Some(result.stderr_string());
    }

    // If step succeeded, run downstream steps that were skipped
    if result.success {
        let schedule = scheduler.resolve(None)?;
        let mut run_remaining = false;

        for batch in &schedule {
            for name in batch {
                if name == &step_name {
                    run_remaining = true;
                    continue;
                }
                if !run_remaining {
                    continue;
                }

                // Check if this step was skipped and depends on the retried step
                if let Some(existing) = state.get_step(name) {
                    if existing.status != StepStatus::Skipped {
                        continue;
                    }
                }

                let downstream_config = pipeline.get_step(name).unwrap();
                let downstream_result = executor.run_step(&pipeline.name, downstream_config).await?;

                let ds_status = if downstream_result.success { StepStatus::Success } else { StepStatus::Failed };

                // Update or add the downstream step
                if state.get_step(name).is_some() {
                    state.update_step(
                        name,
                        ds_status.clone(),
                        Some(downstream_result.exit_code),
                        Some(downstream_result.duration.as_millis() as u64),
                    );
                    if let Some(step) = state.get_step_mut(name) {
                        step.stdout = Some(downstream_result.stdout_string());
                        step.stderr = Some(downstream_result.stderr_string());
                    }
                }

                if !downstream_result.success && !downstream_config.allow_failure {
                    break;
                }
            }
        }
    }

    // Determine overall status
    let all_success = state.steps.iter().all(|s| {
        s.status == StepStatus::Success || s.status == StepStatus::Skipped
    });
    let has_retryable = state.steps.iter().any(|s| {
        s.status == StepStatus::Failed && s.on_failure.as_ref()
            .map(|of| of.strategy == "auto_fix" && of.retries_remaining > 0)
            .unwrap_or(false)
    });

    state.duration_ms = Some(start.elapsed().as_millis() as u64);

    if all_success {
        state.status = PipelineStatus::Success;
    } else if has_retryable {
        state.status = PipelineStatus::Retryable;
    } else {
        state.status = PipelineStatus::Failed;
    }

    state.save(&base_dir)?;

    match output_mode {
        OutputMode::Json => json::print_run_state(&state),
        OutputMode::Plain => plain::print_run_state(&state),
        OutputMode::Tty => {
            let reporter = tty::PipelineReporter::new();
            // Print retry result in TTY mode
            println!("Retry result for step '{}': {:?}", step_name, new_status);
        }
    }

    if all_success {
        Ok(0)
    } else if has_retryable {
        Ok(1)
    } else {
        Ok(2)
    }
}
```

- [ ] **Step 2: Wire dispatch to cmd_retry**

Replace the placeholder in `dispatch()`:

```rust
Command::Retry { run_id, step, output, file } => {
    let mode = resolve_output_mode(output);
    cmd_retry(run_id, step, mode, file).await
}
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/cli/mod.rs
git commit -m "feat: implement retry command with state restoration and downstream execution"
```

---

### Task 9: Implement status command

**Files:**
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Implement cmd_status**

```rust
async fn cmd_status(run_id: String, output_mode: OutputMode) -> Result<i32> {
    let base_dir = RunState::default_base_dir();
    let state = RunState::load(&base_dir, &run_id)
        .context(format!("No run found with id '{}'", run_id))?;

    match output_mode {
        OutputMode::Json => json::print_run_state(&state),
        OutputMode::Plain => plain::print_run_state(&state),
        OutputMode::Tty => plain::print_run_state(&state), // TTY and plain same for status
    }

    Ok(0)
}
```

- [ ] **Step 2: Wire dispatch**

```rust
Command::Status { run_id, output } => {
    let mode = resolve_output_mode(output);
    cmd_status(run_id, mode).await
}
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/cli/mod.rs
git commit -m "feat: implement status command"
```

---

### Task 10: Integration test — full run-retry cycle

**Files:**
- Create: `tests/integration_test.rs`

- [ ] **Step 1: Write integration test for the full cycle**

Create `tests/integration_test.rs`:

```rust
use std::process::Command;

#[test]
fn test_cli_help() {
    let output = Command::new("cargo")
        .args(["run", "--", "--help"])
        .output()
        .expect("Failed to run pipelight");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run"));
    assert!(stdout.contains("retry"));
    assert!(stdout.contains("status"));
}

#[test]
fn test_validate_pipeline() {
    let output = Command::new("cargo")
        .args(["run", "--", "validate", "-f", "pipeline.yml"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
}

#[test]
fn test_list_pipeline() {
    let output = Command::new("cargo")
        .args(["run", "--", "list", "-f", "pipeline.yml"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("build"));
}

#[test]
fn test_json_output_validate() {
    // validate doesn't support --output yet, but run does
    // This test verifies the flag is accepted
    let output = Command::new("cargo")
        .args(["run", "--", "run", "-f", "pipeline.yml", "--output", "json", "--dry-run"])
        .output()
        .expect("Failed to run pipelight");
    // dry-run doesn't produce JSON, but should not error on the flag
    assert!(output.status.success());
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test integration_test`
Expected: all tests PASS (Docker tests skipped if Docker not running)

- [ ] **Step 3: Commit**

```bash
git add tests/
git commit -m "test: add integration tests for CLI commands"
```

---

### Task 11: Update pipeline.yml example and docs

**Files:**
- Modify: `pipeline.yml`
- Modify: `docs/architecture.md`

- [ ] **Step 1: Update pipeline.yml with on_failure examples**

Update `pipeline.yml` to include `on_failure` blocks as shown in the design spec.

- [ ] **Step 2: Update architecture.md**

Update the architecture doc to reflect the new modules (`run_state`, output split, retry/status commands).

- [ ] **Step 3: Commit**

```bash
git add pipeline.yml docs/
git commit -m "docs: update pipeline.yml and architecture with on_failure and retry"
```

---

### Task 12: Final cleanup and push

- [ ] **Step 1: Run all tests**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Fix any warnings.

- [ ] **Step 3: Final commit and push**

```bash
git push
```
