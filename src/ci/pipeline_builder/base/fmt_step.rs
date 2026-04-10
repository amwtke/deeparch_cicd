use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct FmtStep {
    pub image: String,
    pub fmt_cmd: Vec<String>,
    #[allow(dead_code)]
    pub source_paths: Vec<String>,
}

impl FmtStep {
    pub fn new(info: &ProjectInfo) -> Option<Self> {
        info.fmt_cmd.as_ref().map(|cmd| Self {
            image: info.image.clone(),
            fmt_cmd: cmd.clone(),
            source_paths: info.source_paths.clone(),
        })
    }
}

impl StepDef for FmtStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "fmt-check".into(),
            image: self.image.clone(),
            commands: self.fmt_cmd.clone(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix).add(
            "fmt_error",
            ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 1,
                context_paths: self.source_paths.clone(),
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("fmt_error".into())
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        if success {
            "fmt-check: passed".into()
        } else {
            let output = format!("{}{}", stdout, stderr);
            let error_count = count_pattern(&output, &["error:", "Error"]);
            if error_count > 0 {
                format!("fmt-check: {} errors", error_count)
            } else {
                "fmt-check: failed".into()
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
        assert!(FmtStep::new(&info).is_none());
    }

    #[test]
    fn test_config_on_failure() {
        let mut info = make_info();
        info.fmt_cmd = Some(vec!["cargo fmt -- --check".into()]);
        let step = FmtStep::new(&info).unwrap();
        let cfg = step.config();
        assert_eq!(cfg.name, "fmt-check");
    }

    #[test]
    fn test_exception_mapping() {
        use crate::ci::callback::command::CallbackCommand;
        let mut info = make_info();
        info.fmt_cmd = Some(vec!["cargo fmt -- --check".into()]);
        let step = FmtStep::new(&info).unwrap();
        let mapping = step.exception_mapping();
        let resolved = mapping.resolve(
            1,
            "",
            "fmt error output",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 1);
        assert_eq!(resolved.context_paths, vec!["src/"]);
    }

    #[test]
    fn test_report_passed() {
        let mut info = make_info();
        info.fmt_cmd = Some(vec!["cargo fmt -- --check".into()]);
        let step = FmtStep::new(&info).unwrap();
        assert_eq!(step.output_report_str(true, "", ""), "fmt-check: passed");
    }

    #[test]
    fn test_report_errors() {
        let mut info = make_info();
        info.fmt_cmd = Some(vec!["cargo fmt -- --check".into()]);
        let step = FmtStep::new(&info).unwrap();
        assert_eq!(
            step.output_report_str(
                false,
                "error: left behind trailing whitespace\nerror: diff\n",
                ""
            ),
            "fmt-check: 2 errors"
        );
    }

    #[test]
    fn test_report_failed_no_errors() {
        let mut info = make_info();
        info.fmt_cmd = Some(vec!["cargo fmt -- --check".into()]);
        let step = FmtStep::new(&info).unwrap();
        assert_eq!(
            step.output_report_str(false, "Diff in foo.rs\n", ""),
            "fmt-check: failed"
        );
    }
}
