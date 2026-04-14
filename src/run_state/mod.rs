use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::ci::callback::action::CallbackCommandAction;
use crate::ci::callback::command::CallbackCommand;
use crate::ci::pipeline_builder::test_parser::TestSummary;

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
    pub exception_key: String,
    pub command: CallbackCommand,
    pub action: CallbackCommandAction,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_summary: Option<TestSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    pub pipeline: String,
    pub status: PipelineStatus,
    pub duration_ms: Option<u64>,
    pub steps: Vec<StepState>,
    /// Full-scan mode flag set by `pipelight run --full`; persisted so retries
    /// inherit the same scan semantics.
    #[serde(default)]
    pub full: bool,
}

impl RunState {
    pub fn new(run_id: &str, pipeline_name: &str) -> Self {
        Self {
            run_id: run_id.to_string(),
            pipeline: pipeline_name.to_string(),
            status: PipelineStatus::Running,
            duration_ms: None,
            steps: Vec::new(),
            full: false,
        }
    }

    pub fn add_step(&mut self, step: StepState) {
        self.steps.push(step);
    }

    pub fn get_step(&self, name: &str) -> Option<&StepState> {
        self.steps.iter().find(|s| s.name == name)
    }

    pub fn get_step_mut(&mut self, name: &str) -> Option<&mut StepState> {
        self.steps.iter_mut().find(|s| s.name == name)
    }

    #[allow(dead_code)]
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

    fn run_dir(base: &Path, run_id: &str) -> PathBuf {
        base.join(run_id)
    }

    pub fn save(&self, base: &Path) -> Result<()> {
        let dir = Self::run_dir(base, &self.run_id);
        std::fs::create_dir_all(&dir)
            .context(format!("Failed to create run directory: {}", dir.display()))?;
        let path = dir.join("status.json");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json).context(format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn load(base: &Path, run_id: &str) -> Result<Self> {
        let path = Self::run_dir(base, run_id).join("status.json");
        let content =
            std::fs::read_to_string(&path).context(format!("Failed to read {}", path.display()))?;
        let state: Self = serde_json::from_str(&content).context("Failed to parse status.json")?;
        Ok(state)
    }

    pub fn default_base_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".pipelight")
            .join("runs")
    }

    /// Mark all unexecuted steps from `schedule[batch_index..]` as Skipped.
    /// Steps already present in state are skipped (they were executed before the failure).
    /// `step_info_fn` provides (image, command) for each step name.
    pub fn mark_unexecuted_as_skipped<F>(
        &mut self,
        schedule: &[Vec<String>],
        batch_index: usize,
        step_info_fn: F,
    ) where
        F: Fn(&str) -> (String, String),
    {
        for batch in &schedule[batch_index..] {
            for step_name in batch {
                if self.get_step(step_name).is_some() {
                    continue;
                }
                let (image, command) = step_info_fn(step_name);
                self.add_step(StepState {
                    name: step_name.clone(),
                    status: StepStatus::Skipped,
                    exit_code: None,
                    duration_ms: None,
                    image,
                    command,
                    stdout: None,
                    stderr: None,
                    error_context: None,
                    on_failure: None,
                    test_summary: None,
                    report_summary: None,
                    report_path: None,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::callback::action::CallbackCommandAction;
    use crate::ci::callback::command::CallbackCommand;

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
        state.add_step(StepState {
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
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        state.update_step("build", StepStatus::Failed, Some(101), Some(8200));
        let step = state.get_step("build").unwrap();
        assert_eq!(step.status, StepStatus::Failed);
        assert_eq!(step.exit_code, Some(101));
    }

    #[test]
    fn test_retries_remaining() {
        let mut state = RunState::new("run-1", "pipeline-1");
        state.add_step(StepState {
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
                exception_key: "compile_error".into(),
                command: CallbackCommand::AutoFix,
                action: CallbackCommandAction::Retry,
                max_retries: 3,
                retries_remaining: 3,
                context_paths: vec!["src/".into()],
            }),
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        state.decrement_retries("build");
        let step = state.get_step("build").unwrap();
        assert_eq!(step.on_failure.as_ref().unwrap().retries_remaining, 2);
    }

    #[test]
    fn test_load_nonexistent_fails() {
        let dir = tempfile::tempdir().unwrap();
        let result = RunState::load(dir.path(), "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_save_load_roundtrip_with_steps() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = RunState::new("roundtrip-1", "my-pipeline");
        state.status = PipelineStatus::Retryable;
        state.duration_ms = Some(5000);
        state.add_step(StepState {
            name: "build".into(),
            status: StepStatus::Success,
            exit_code: Some(0),
            duration_ms: Some(2000),
            image: "rust:1.78".into(),
            command: "cargo build".into(),
            stdout: Some("compiled OK".into()),
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });
        state.add_step(StepState {
            name: "test".into(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            duration_ms: Some(3000),
            image: "rust:1.78".into(),
            command: "cargo test".into(),
            stdout: None,
            stderr: Some("assertion failed".into()),
            error_context: Some(ErrorContext {
                files: vec!["src/lib.rs".into()],
                lines: vec![42],
                error_type: "test_failure".into(),
            }),
            on_failure: Some(OnFailureState {
                exception_key: "compile_error".into(),
                command: CallbackCommand::AutoFix,
                action: CallbackCommandAction::Retry,
                max_retries: 3,
                retries_remaining: 2,
                context_paths: vec!["src/".into()],
            }),
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        state.save(dir.path()).unwrap();
        let loaded = RunState::load(dir.path(), "roundtrip-1").unwrap();

        assert_eq!(loaded.run_id, "roundtrip-1");
        assert_eq!(loaded.pipeline, "my-pipeline");
        assert_eq!(loaded.status, PipelineStatus::Retryable);
        assert_eq!(loaded.duration_ms, Some(5000));
        assert_eq!(loaded.steps.len(), 2);

        let build = loaded.get_step("build").unwrap();
        assert_eq!(build.status, StepStatus::Success);
        assert_eq!(build.stdout, Some("compiled OK".into()));

        let test = loaded.get_step("test").unwrap();
        assert_eq!(test.status, StepStatus::Failed);
        assert_eq!(test.stderr, Some("assertion failed".into()));
        let ec = test.error_context.as_ref().unwrap();
        assert_eq!(ec.files, vec!["src/lib.rs"]);
        assert_eq!(ec.lines, vec![42]);
        let of = test.on_failure.as_ref().unwrap();
        assert_eq!(of.retries_remaining, 2);
    }

    #[test]
    fn test_decrement_retries_to_zero() {
        let mut state = RunState::new("run-1", "p");
        state.add_step(StepState {
            name: "s".into(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            duration_ms: None,
            image: "alpine".into(),
            command: "exit 1".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: Some(OnFailureState {
                exception_key: "compile_error".into(),
                command: CallbackCommand::AutoFix,
                action: CallbackCommandAction::Retry,
                max_retries: 1,
                retries_remaining: 1,
                context_paths: vec![],
            }),
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        state.decrement_retries("s");
        assert_eq!(
            state
                .get_step("s")
                .unwrap()
                .on_failure
                .as_ref()
                .unwrap()
                .retries_remaining,
            0
        );

        // Decrementing at 0 should stay at 0
        state.decrement_retries("s");
        assert_eq!(
            state
                .get_step("s")
                .unwrap()
                .on_failure
                .as_ref()
                .unwrap()
                .retries_remaining,
            0
        );
    }

    #[test]
    fn test_decrement_retries_no_on_failure() {
        let mut state = RunState::new("run-1", "p");
        state.add_step(StepState {
            name: "s".into(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            duration_ms: None,
            image: "alpine".into(),
            command: "exit 1".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });
        // Should not panic
        state.decrement_retries("s");
        assert!(state.get_step("s").unwrap().on_failure.is_none());
    }

    #[test]
    fn test_skip_action_serialization_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = RunState::new("skip-run-1", "my-pipeline");
        state.status = PipelineStatus::Success;
        state.add_step(StepState {
            name: "git-pull".into(),
            status: StepStatus::Skipped,
            exit_code: Some(1),
            duration_ms: Some(500),
            image: "".into(),
            command: "git pull --rebase".into(),
            stdout: None,
            stderr: Some("fatal: SSL error".into()),
            error_context: None,
            on_failure: Some(OnFailureState {
                exception_key: "unrecognized".into(),
                command: CallbackCommand::FailAndSkip,
                action: CallbackCommandAction::Skip,
                max_retries: 0,
                retries_remaining: 0,
                context_paths: vec![],
            }),
            test_summary: None,
            report_summary: Some("git-pull skipped".into()),
            report_path: None,
        });
        state.add_step(StepState {
            name: "build".into(),
            status: StepStatus::Success,
            exit_code: Some(0),
            duration_ms: Some(3000),
            image: "maven:3.9".into(),
            command: "mvn compile".into(),
            stdout: Some("BUILD SUCCESS".into()),
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        state.save(dir.path()).unwrap();
        let loaded = RunState::load(dir.path(), "skip-run-1").unwrap();

        let git_pull = loaded.get_step("git-pull").unwrap();
        assert_eq!(git_pull.status, StepStatus::Skipped);
        let of = git_pull.on_failure.as_ref().unwrap();
        assert_eq!(of.command, CallbackCommand::FailAndSkip);
        assert_eq!(of.action, CallbackCommandAction::Skip);
        assert_eq!(of.max_retries, 0);

        let build = loaded.get_step("build").unwrap();
        assert_eq!(build.status, StepStatus::Success);
    }

    #[test]
    fn test_pipeline_success_with_skipped_steps() {
        let mut state = RunState::new("run-skip", "p");
        state.add_step(StepState {
            name: "git-pull".into(),
            status: StepStatus::Skipped,
            exit_code: Some(1),
            duration_ms: None,
            image: "".into(),
            command: "git pull".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });
        state.add_step(StepState {
            name: "build".into(),
            status: StepStatus::Success,
            exit_code: Some(0),
            duration_ms: None,
            image: "rust:1.78".into(),
            command: "cargo build".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        // Skipped + Success should count as all_success
        let all_success = state
            .steps
            .iter()
            .all(|s| s.status == StepStatus::Success || s.status == StepStatus::Skipped);
        assert!(all_success);
    }

    #[test]
    fn test_pipeline_not_success_with_failed_and_skipped() {
        let mut state = RunState::new("run-fail", "p");
        state.add_step(StepState {
            name: "git-pull".into(),
            status: StepStatus::Skipped,
            exit_code: Some(1),
            duration_ms: None,
            image: "".into(),
            command: "git pull".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });
        state.add_step(StepState {
            name: "build".into(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            duration_ms: None,
            image: "rust:1.78".into(),
            command: "cargo build".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        // Skipped + Failed should NOT count as all_success
        let all_success = state
            .steps
            .iter()
            .all(|s| s.status == StepStatus::Success || s.status == StepStatus::Skipped);
        assert!(!all_success);
    }

    #[test]
    fn test_default_base_dir() {
        let base = RunState::default_base_dir();
        assert!(base.to_string_lossy().contains(".pipelight"));
        assert!(base.to_string_lossy().contains("runs"));
    }

    #[test]
    fn test_pipeline_status_serialization() {
        let state = RunState::new("ser-1", "p");
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"running\""));

        let mut state2 = RunState::new("ser-2", "p");
        state2.status = PipelineStatus::Retryable;
        let json2 = serde_json::to_string(&state2).unwrap();
        assert!(json2.contains("\"retryable\""));
    }

    // --- mark_unexecuted_as_skipped tests ---

    fn info_fn(name: &str) -> (String, String) {
        (format!("img-{name}"), format!("cmd-{name}"))
    }

    #[test]
    fn test_mark_unexecuted_skips_steps_already_in_state() {
        // Simulate: batch 0 = [git-pull], batch 1 = [fmt-check, build]
        // fmt-check executed and failed, build was NOT executed.
        let mut state = RunState::new("run-1", "p");
        state.add_step(StepState {
            name: "git-pull".into(),
            status: StepStatus::Success,
            exit_code: Some(0),
            duration_ms: Some(100),
            image: "".into(),
            command: "git pull".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });
        state.add_step(StepState {
            name: "fmt-check".into(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            duration_ms: Some(200),
            image: "rust:latest".into(),
            command: "cargo fmt -- --check".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        let schedule = vec![
            vec!["git-pull".into()],
            vec!["fmt-check".into(), "build".into()],
            vec!["test".into(), "clippy".into()],
        ];

        // batch_index=1 means we start from the batch where failure occurred
        state.mark_unexecuted_as_skipped(&schedule, 1, info_fn);

        // git-pull: already in state, should NOT be duplicated
        assert_eq!(
            state.steps.iter().filter(|s| s.name == "git-pull").count(),
            1
        );
        // fmt-check: already in state as Failed, should NOT be overwritten
        assert_eq!(
            state.get_step("fmt-check").unwrap().status,
            StepStatus::Failed
        );
        // build: was NOT in state, should now be Skipped
        let build = state.get_step("build").unwrap();
        assert_eq!(build.status, StepStatus::Skipped);
        assert_eq!(build.image, "img-build");
        assert_eq!(build.command, "cmd-build");
        // test & clippy: next batch, should be Skipped
        assert_eq!(state.get_step("test").unwrap().status, StepStatus::Skipped);
        assert_eq!(
            state.get_step("clippy").unwrap().status,
            StepStatus::Skipped
        );
        // Total steps: git-pull + fmt-check + build + test + clippy = 5
        assert_eq!(state.steps.len(), 5);
    }

    #[test]
    fn test_mark_unexecuted_all_in_state_no_duplicates() {
        // All steps already executed before failure in same batch
        let mut state = RunState::new("run-2", "p");
        state.add_step(StepState {
            name: "a".into(),
            status: StepStatus::Success,
            exit_code: Some(0),
            duration_ms: None,
            image: "".into(),
            command: "".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });
        state.add_step(StepState {
            name: "b".into(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            duration_ms: None,
            image: "".into(),
            command: "".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        let schedule = vec![vec!["a".into(), "b".into()]];
        state.mark_unexecuted_as_skipped(&schedule, 0, info_fn);

        // No new steps should be added
        assert_eq!(state.steps.len(), 2);
    }

    #[test]
    fn test_mark_unexecuted_empty_schedule_after_index() {
        let mut state = RunState::new("run-3", "p");
        state.add_step(StepState {
            name: "only".into(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            duration_ms: None,
            image: "".into(),
            command: "".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        // Single batch, failure at last step — nothing more to mark
        let schedule = vec![vec!["only".into()]];
        state.mark_unexecuted_as_skipped(&schedule, 0, info_fn);
        assert_eq!(state.steps.len(), 1);
    }

    #[test]
    fn test_mark_unexecuted_multi_batch_first_step_fails() {
        // Simulate: batch 0 has 4 parallel steps (like Go/Node/Python),
        // first step fails retryable, remaining 3 unexecuted.
        let mut state = RunState::new("run-4", "p");
        state.add_step(StepState {
            name: "vet".into(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            duration_ms: None,
            image: "go:latest".into(),
            command: "go vet".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
            report_summary: None,
            report_path: None,
        });

        let schedule = vec![
            vec![
                "vet".into(),
                "lint".into(),
                "test".into(),
                "fmt-check".into(),
            ],
            vec!["package".into()],
        ];

        state.mark_unexecuted_as_skipped(&schedule, 0, info_fn);

        // vet: already Failed, untouched
        assert_eq!(state.get_step("vet").unwrap().status, StepStatus::Failed);
        // lint, test, fmt-check: now Skipped
        assert_eq!(state.get_step("lint").unwrap().status, StepStatus::Skipped);
        assert_eq!(state.get_step("test").unwrap().status, StepStatus::Skipped);
        assert_eq!(
            state.get_step("fmt-check").unwrap().status,
            StepStatus::Skipped
        );
        // package: next batch, also Skipped
        assert_eq!(
            state.get_step("package").unwrap().status,
            StepStatus::Skipped
        );
        assert_eq!(state.steps.len(), 5);
    }
}
