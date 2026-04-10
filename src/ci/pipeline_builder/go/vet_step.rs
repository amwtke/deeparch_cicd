use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct VetStep {
    image: String,
    source_paths: Vec<String>,
}

impl VetStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
        }
    }
}

impl StepDef for VetStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "vet".into(),
            image: self.image.clone(),
            commands: vec!["go vet ./...".into()],
            depends_on: vec!["build".into()],
            on_failure: None,
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add("vet_error", ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("vet_error".into())
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let issues = count_pattern(&output, &["vet:", "error", "Error"]);
        if success {
            "vet: passed".into()
        } else {
            if issues > 0 {
                format!("vet: {} issues found", issues)
            } else {
                "vet: failed".into()
            }
        }
    }
}
