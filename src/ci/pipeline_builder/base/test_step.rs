use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{StepConfig, StepDef};

pub struct TestStep {
    pub image: String,
    pub test_cmd: Vec<String>,
    pub test_parser: Option<fn(&str) -> Option<String>>,
    pub allow_failure: bool,
    pub callback_command: CallbackCommand,
    /// Glob patterns (relative to project root) where per-module JUnit-style
    /// XML reports land. Handed to the LLM via `test_print` callback so it can
    /// aggregate them into a table without guessing the build system layout.
    pub test_report_globs: Vec<String>,
    /// Substrings whose presence in stdout/stderr signals a real failure even
    /// when the parser couldn't extract counts (e.g. Maven's `BUILD FAILURE`,
    /// Gradle's `FAILURE:`). Only consulted in `allow_failure` mode where the
    /// executor reports `success=true` regardless of build outcome.
    pub failure_markers: Vec<String>,
    /// Summary string returned when `failure_markers` matches.
    pub failure_marker_summary: String,
}

impl TestStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            test_cmd: info.test_cmd.clone(),
            test_parser: None,
            allow_failure: false,
            callback_command: CallbackCommand::Abort,
            test_report_globs: vec![],
            failure_markers: vec![],
            failure_marker_summary: String::new(),
        }
    }

    /// Configure substrings that indicate a real failure when the parser
    /// returns nothing. Used by Maven/Gradle whose `allow_failure=true` mode
    /// makes the executor report success even on `BUILD FAILURE`.
    pub fn with_failure_markers(
        mut self,
        markers: Vec<String>,
        summary: impl Into<String>,
    ) -> Self {
        self.failure_markers = markers;
        self.failure_marker_summary = summary.into();
        self
    }

    pub fn with_parser(mut self, parser: fn(&str) -> Option<String>) -> Self {
        self.test_parser = Some(parser);
        self
    }

    /// When true, the step may fail without aborting the pipeline (report-only mode).
    pub fn with_allow_failure(mut self, allow: bool) -> Self {
        self.allow_failure = allow;
        self
    }

    /// Override the default callback command for test failures.
    #[allow(dead_code)]
    pub fn with_callback_command(mut self, cmd: CallbackCommand) -> Self {
        self.callback_command = cmd;
        self
    }

    /// Set glob patterns for JUnit-style XML reports.
    pub fn with_test_report_globs(mut self, globs: Vec<String>) -> Self {
        self.test_report_globs = globs;
        self
    }
}

impl StepDef for TestStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "test".into(),
            image: self.image.clone(),
            commands: self.test_cmd.clone(),
            depends_on: vec!["build".into()],
            allow_failure: self.allow_failure,
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        let mut mapping = ExceptionMapping::new(self.callback_command.clone());
        // Always fire `test_print`: when JUnit globs are configured (Maven/Gradle),
        // hand them as context_paths for per-module aggregation; otherwise leave
        // context_paths empty and the LLM falls back to the step's `report_path`
        // (raw log) to extract pass/fail/skip counts for runners that don't emit
        // JUnit XML (cargo test, go test, jest --no-reporter, pytest, etc.).
        let entry = ExceptionEntry {
            command: CallbackCommand::TestPrintCommand,
            max_retries: 0,
            context_paths: self.test_report_globs.clone(),
        };
        if self.allow_failure {
            mapping = mapping.add("test_failures", entry.clone());
        }
        mapping = mapping.add("test_success", entry);
        mapping
    }

    fn match_exception(&self, exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        if exit_code == 0 {
            // Always surface a test_print callback on success so the LLM prints
            // the summary table (from JUnit globs or the log file fallback).
            return Some("test_success".into());
        }
        let output = format!("{}{}", stdout, stderr);
        let looks_failed = output.contains("BUILD FAILED")
            || output.contains("BUILD FAILURE")
            || output.contains("FAILURE:")
            || output.contains("There were failing tests")
            || output.contains("There are test failures");
        if looks_failed && self.allow_failure {
            Some("test_failures".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if let Some(parser) = self.test_parser {
            if let Some(report) = parser(&output) {
                return report;
            }
        }
        if !self.failure_markers.is_empty()
            && self
                .failure_markers
                .iter()
                .any(|m| output.contains(m.as_str()))
        {
            return self.failure_marker_summary.clone();
        }
        if success {
            "Tests passed".into()
        } else {
            "Tests failed".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::callback::command::CallbackCommand;
    use crate::ci::detector::{ProjectInfo, ProjectType};

    fn make_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: None,
            framework: None,
            image: "rust:latest".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_config() {
        let step = TestStep::new(&make_info());
        let cfg = step.config();
        assert_eq!(cfg.name, "test");
        assert_eq!(cfg.depends_on, vec!["build"]);
    }

    #[test]
    fn test_exception_mapping() {
        let step = TestStep::new(&make_info());
        let resolved = step
            .exception_mapping()
            .resolve(1, "", "some test failure", None);
        assert_eq!(resolved.command, CallbackCommand::Abort);
        assert_eq!(resolved.max_retries, 0);
    }

    #[test]
    fn test_generic_report() {
        let step = TestStep::new(&make_info());
        assert_eq!(step.output_report_str(true, "ok", ""), "Tests passed");
        assert_eq!(step.output_report_str(false, "FAIL", ""), "Tests failed");
    }

    #[test]
    fn test_custom_parser() {
        fn my_parser(output: &str) -> Option<String> {
            if output.contains("42 passed") {
                Some("42 tests passed".into())
            } else {
                None
            }
        }

        let step = TestStep::new(&make_info()).with_parser(my_parser);
        assert_eq!(
            step.output_report_str(true, "42 passed", ""),
            "42 tests passed"
        );
        // Falls back to generic when parser returns None
        assert_eq!(
            step.output_report_str(true, "some other output", ""),
            "Tests passed"
        );
    }

    #[test]
    fn test_failure_markers_surface_when_parser_returns_none() {
        // When the parser can't extract counts (e.g. surefire produced no XML
        // because the build broke early), failure_markers let Maven/Gradle
        // surface "Tests had failures (report-only)" instead of misleading
        // "Tests passed" under allow_failure mode.
        let step = TestStep::new(&make_info()).with_failure_markers(
            vec!["BUILD FAILURE".into()],
            "Tests had failures (report-only)",
        );
        assert_eq!(
            step.output_report_str(true, "[INFO] BUILD FAILURE", ""),
            "Tests had failures (report-only)"
        );
    }

    #[test]
    fn test_failure_markers_ignored_when_no_match() {
        // No marker in output → fall through to success/failure default.
        let step = TestStep::new(&make_info())
            .with_failure_markers(vec!["BUILD FAILURE".into()], "Tests had failures");
        assert_eq!(
            step.output_report_str(true, "[INFO] BUILD SUCCESS", ""),
            "Tests passed"
        );
    }

    #[test]
    fn test_failure_markers_yield_to_parser() {
        // When the parser succeeds, failure_markers must NOT override its
        // result — the parsed counts are richer information.
        fn parser(_: &str) -> Option<String> {
            Some("42 passed, 0 failed, 0 skipped".into())
        }
        let step = TestStep::new(&make_info())
            .with_parser(parser)
            .with_failure_markers(vec!["BUILD FAILURE".into()], "Tests had failures");
        assert_eq!(
            step.output_report_str(true, "BUILD FAILURE noise", ""),
            "42 passed, 0 failed, 0 skipped"
        );
    }

    #[test]
    fn test_allow_failure_default_false() {
        let step = TestStep::new(&make_info());
        assert!(!step.config().allow_failure);
    }

    #[test]
    fn test_allow_failure_true() {
        let step = TestStep::new(&make_info()).with_allow_failure(true);
        assert!(step.config().allow_failure);
    }

    #[test]
    fn test_custom_callback_command() {
        let step = TestStep::new(&make_info()).with_callback_command(CallbackCommand::RuntimeError);
        let resolved = step.exception_mapping().resolve(1, "", "failure", None);
        assert_eq!(resolved.command, CallbackCommand::RuntimeError);
    }
}
