use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct LintStep {
    pub image: String,
    pub lint_cmd: Vec<String>,
    #[allow(dead_code)]
    pub source_paths: Vec<String>,
}

impl LintStep {
    pub fn new(info: &ProjectInfo) -> Option<Self> {
        info.lint_cmd.as_ref().map(|cmd| Self {
            image: info.image.clone(),
            lint_cmd: cmd.clone(),
            source_paths: info.source_paths.clone(),
        })
    }
}

impl StepDef for LintStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "lint".into(),
            image: self.image.clone(),
            commands: self.lint_cmd.clone(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix).add(
            "lint_error",
            ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 9,
                context_paths: self.source_paths.clone(),
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("lint_error".into())
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let warning_count = count_pattern(&output, &["warning:", "WARNING", "[WARN]"]);
        let violation_count = count_pattern(&output, &["violation", "Violation"]);
        let issues = warning_count + violation_count;
        if success {
            if issues > 0 {
                format!("lint: passed ({} warnings)", issues)
            } else {
                "lint: no issues found".into()
            }
        } else {
            if issues > 0 {
                format!("lint: {} issues found", issues)
            } else {
                "lint: failed".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_none_when_no_cmd() {
        let info = make_info();
        assert!(LintStep::new(&info).is_none());
    }

    #[test]
    fn test_config_on_failure() {
        let mut info = make_info();
        info.lint_cmd = Some(vec!["cargo clippy".into()]);
        let step = LintStep::new(&info).unwrap();
        let cfg = step.config();
        assert_eq!(cfg.name, "lint");
    }

    #[test]
    fn test_exception_mapping() {
        use crate::ci::callback::command::CallbackCommand;
        let mut info = make_info();
        info.lint_cmd = Some(vec!["cargo clippy".into()]);
        let step = LintStep::new(&info).unwrap();
        let mapping = step.exception_mapping();
        let resolved = mapping.resolve(
            1,
            "",
            "lint error output",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 9);
        assert_eq!(resolved.context_paths, vec!["src/"]);
    }

    #[test]
    fn test_report_clean() {
        let mut info = make_info();
        info.lint_cmd = Some(vec!["cargo clippy".into()]);
        let step = LintStep::new(&info).unwrap();
        assert_eq!(
            step.output_report_str(true, "Finished dev profile\n", ""),
            "lint: no issues found"
        );
    }
}
