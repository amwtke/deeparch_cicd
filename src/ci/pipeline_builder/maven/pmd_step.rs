use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, CallbackCommand};
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
        // Check for custom ruleset in pipelight-misc.
        // If found: run mvn pmd:pmd with that ruleset, collect per-module reports, output summary.
        // If not found: emit callback marker and exit 1 so the LLM can search/generate a ruleset.
        let cmd = format!(
            "{cd}if [ -f /workspace/pipelight-misc/pmd-ruleset.xml ]; then \
             mvn pmd:pmd -Dpmd.rulesetfiles=/workspace/pipelight-misc/pmd-ruleset.xml && \
             mkdir -p /workspace/pipelight-misc/pmd-report && \
             TOTAL=0 && \
             for f in $(find . -path '*/target/pmd.xml' -type f 2>/dev/null); do \
               MODULE=$(echo \"$f\" | sed 's|^\\./||;s|/target/pmd\\.xml||'); \
               cp \"$f\" /workspace/pipelight-misc/pmd-report/\"$(echo $MODULE | tr / _)-pmd.xml\"; \
               COUNT=$(grep -c '<violation' \"$f\" 2>/dev/null || echo 0); \
               if [ \"$COUNT\" -gt 0 ]; then echo \"  $MODULE: $COUNT violations\"; fi; \
               TOTAL=$((TOTAL + COUNT)); \
             done && \
             echo \"\" && echo \"PMD Total: $TOTAL violations\" && \
             if [ \"$TOTAL\" -gt 0 ]; then exit 1; fi; \
             else \
             echo 'PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - No pmd-ruleset.xml found in pipelight-misc/. LLM should search project for existing ruleset or coding guidelines to generate one.' >&2 && exit 1; \
             fi",
            cd = cd_prefix
        );
        StepConfig {
            name: "pmd".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            on_failure: Some(OnFailure {
                callback_command: CallbackCommand::AutoGenPmdRuleset,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            return "pmd: ruleset not found (callback)".into();
        }
        let violations = count_pattern(&output, &["violation", "Violation"]);
        if success { "pmd: no violations".into() }
        else if violations > 0 { format!("pmd: {} violations", violations) }
        else { "pmd: failed".into() }
    }
}
