use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct VueBuildStep {
    image: String,
    commands: Vec<String>,
    depends_on: Vec<String>,
    context_paths: Vec<String>,
}

impl VueBuildStep {
    pub fn new(info: &ProjectInfo, depends_on: Vec<String>) -> Self {
        Self {
            image: info.image.clone(),
            commands: info.build_cmd.clone(),
            depends_on,
            context_paths: super::vue_build_context_paths(info),
        }
    }
}

impl StepDef for VueBuildStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "build".into(),
            image: self.image.clone(),
            commands: self.commands.clone(),
            depends_on: self.depends_on.clone(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix).add(
            "compile_error",
            ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 2,
                context_paths: self.context_paths.clone(),
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("compile_error".into())
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let warning_count = count_pattern(&output, &["warning:", "WARNING", "[WARNING]"]);
        if success {
            if warning_count > 0 {
                format!("Build succeeded ({} warnings)", warning_count)
            } else {
                "Build succeeded".into()
            }
        } else {
            let error_count = count_pattern(&output, &["error:", "ERROR", "[ERROR]"]);
            if error_count > 0 {
                format!("Build failed ({} errors)", error_count)
            } else {
                "Build failed".into()
            }
        }
    }
}
