use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct ClippyStep {
    image: String,
    #[allow(dead_code)]
    source_paths: Vec<String>,
}

impl ClippyStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
        }
    }
}

impl StepDef for ClippyStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "clippy".into(),
            image: self.image.clone(),
            commands: vec![
                "rustup component add clippy 2>/dev/null; cargo clippy -- -D warnings".into(),
            ],
            depends_on: vec!["build".into()],
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add("clippy_error", ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("clippy_error".into())
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let warnings = count_pattern(&output, &["warning:", "WARNING"]);
        if success {
            if warnings > 0 {
                format!("clippy: {} warnings", warnings)
            } else {
                "clippy: no issues found".into()
            }
        } else {
            if warnings > 0 {
                format!("clippy: {} issues found", warnings)
            } else {
                "clippy: failed".into()
            }
        }
    }
}
