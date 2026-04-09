use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{CallbackCommand, OnFailure};
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct ClippyStep {
    image: String,
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
            on_failure: Some(OnFailure {
                callback_command: CallbackCommand::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            }),
            ..Default::default()
        }
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
