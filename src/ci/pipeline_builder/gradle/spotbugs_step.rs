use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, CallbackCommand};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

pub struct SpotbugsStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
}

impl SpotbugsStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for SpotbugsStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{}./gradlew spotbugsMain && cp -r build/reports/spotbugs /workspace/pipelight-misc/spotbugs-report 2>/dev/null || true",
            cd_prefix
        );
        StepConfig {
            name: "spotbugs".into(),
            image: self.image.clone(),
            commands: vec![cmd],
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
        let bugs = count_pattern(&output, &["Bug", "bug"]);
        if success { "spotbugs: no bugs found".into() }
        else if bugs > 0 { format!("spotbugs: {} bugs found", bugs) }
        else { "spotbugs: failed".into() }
    }
}
