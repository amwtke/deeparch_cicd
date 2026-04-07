use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

pub struct PmdStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
}

impl PmdStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for PmdStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{}if [ -f /workspace/pipelight-misc/pmd-ruleset.xml ]; then \
             mvn pmd:pmd -Dpmd.rulesetfiles=/workspace/pipelight-misc/pmd-ruleset.xml \
             -Dpmd.outputDirectory=/workspace/pipelight-misc/pmd-report; \
             else mvn pmd:pmd \
             -Dpmd.outputDirectory=/workspace/pipelight-misc/pmd-report; fi",
            cd_prefix
        );
        StepConfig {
            name: "pmd".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let violations = count_pattern(&output, &["violation", "Violation"]);
        if success { "pmd: no violations".into() }
        else if violations > 0 { format!("pmd: {} violations", violations) }
        else { "pmd: failed".into() }
    }
}
