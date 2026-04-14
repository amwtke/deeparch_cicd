use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct TypecheckStep {
    image: String,
    #[allow(dead_code)]
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
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix).add(
            "typecheck_error",
            ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 9,
                context_paths: self.source_paths.clone(),
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("typecheck_error".into())
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
