use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct CheckstyleStep {
    image: String,
    config_files: Vec<String>,
    subdir: Option<String>,
}

impl CheckstyleStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            config_files: info.config_files.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for CheckstyleStep {
    fn config(&self) -> StepConfig {
        let cmd = match &self.subdir {
            Some(subdir) => format!("cd {} && mvn checkstyle:check", subdir),
            None => "mvn checkstyle:check".into(),
        };
        StepConfig {
            name: "checkstyle".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add("checkstyle_error", ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 2,
                context_paths: self.config_files.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("checkstyle_error".into())
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let violations = count_pattern(&output, &["violation", "Violation", "[WARN]", "WARNING"]);
        if success {
            if violations > 0 {
                format!("checkstyle: passed ({} warnings)", violations)
            } else {
                "checkstyle: no issues found".into()
            }
        } else {
            if violations > 0 {
                format!("checkstyle: {} issues found", violations)
            } else {
                "checkstyle: failed".into()
            }
        }
    }
}
