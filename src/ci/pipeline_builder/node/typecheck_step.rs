use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{CallbackCommand, OnFailure};
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct TypecheckStep {
    image: String,
    source_paths: Vec<String>,
}

impl TypecheckStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
        }
    }
}

impl StepDef for TypecheckStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "typecheck".into(),
            image: self.image.clone(),
            commands: vec!["npx tsc --noEmit".into()],
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
        let errors = count_pattern(&output, &["error TS", "error:", "Error"]);
        if success {
            "typecheck: passed".into()
        } else {
            if errors > 0 {
                format!("typecheck: {} errors", errors)
            } else {
                "typecheck: failed".into()
            }
        }
    }
}
