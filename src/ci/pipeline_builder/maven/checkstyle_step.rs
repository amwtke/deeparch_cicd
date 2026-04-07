use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

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
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 2,
                context_paths: self.config_files.clone(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let violations = count_pattern(&output, &["violation", "Violation", "[WARN]", "WARNING"]);
        if success {
            if violations > 0 { format!("checkstyle: passed ({} warnings)", violations) }
            else { "checkstyle: no issues found".into() }
        } else {
            if violations > 0 { format!("checkstyle: {} issues found", violations) }
            else { "checkstyle: failed".into() }
        }
    }
}
