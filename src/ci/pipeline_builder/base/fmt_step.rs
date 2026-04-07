use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, CallbackCommand};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

pub struct FmtStep {
    pub image: String,
    pub fmt_cmd: Vec<String>,
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
            on_failure: Some(OnFailure {
                callback_command: CallbackCommand::AutoFix,
                max_retries: 1,
                context_paths: self.source_paths.clone(),
            }),
            ..Default::default()
        }
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
        let of = cfg.on_failure.unwrap();
        assert_eq!(of.callback_command, CallbackCommand::AutoFix);
        assert_eq!(of.max_retries, 1);
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
            step.output_report_str(false, "error: left behind trailing whitespace\nerror: diff\n", ""),
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
