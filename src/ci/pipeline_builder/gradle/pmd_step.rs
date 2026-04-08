use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, CallbackCommand};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

/// PMD version used for standalone CLI fallback when the Gradle PMD plugin is unavailable.
const PMD_CLI_VERSION: &str = "7.9.0";

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
        // PMD step with two execution paths:
        //
        // Path A: Ruleset exists → run PMD
        //   1. Check if Gradle PMD plugin is available (./gradlew pmdMain --dry-run)
        //   2a. Plugin exists → configure ruleset via init script, run ./gradlew pmdMain
        //   2b. Plugin missing → download standalone PMD CLI, scan all src/main/java dirs
        //
        // Path B: No ruleset → emit callback for LLM to search/generate one
        let cmd = format!(
            "{cd}if [ -f /workspace/pipelight-misc/pmd-ruleset.xml ]; then \
             mkdir -p /workspace/pipelight-misc/pmd-report && \
             if ./gradlew pmdMain --dry-run > /dev/null 2>&1; then \
               cat > /tmp/pmd-init.gradle << 'INITEOF'\n\
allprojects {{\n\
  plugins.withId('pmd') {{\n\
    pmd {{\n\
      ruleSetFiles = files('/workspace/pipelight-misc/pmd-ruleset.xml')\n\
      ruleSets = []\n\
    }}\n\
  }}\n\
}}\n\
INITEOF\n\
               ./gradlew --init-script /tmp/pmd-init.gradle pmdMain && \
               find . -path '*/build/reports/pmd/*.xml' -type f -exec cp {{}} /workspace/pipelight-misc/pmd-report/ \\; 2>/dev/null; \
             else \
               echo 'PMD plugin not found in Gradle, using standalone PMD CLI...' && \
               PMD_CACHE=/root/.pipelight/cache && \
               PMD_DIR=$PMD_CACHE/pmd-bin-{pmd_ver} && \
               if [ ! -f $PMD_DIR/bin/pmd ]; then \
                 mkdir -p $PMD_CACHE && \
                 curl -sL https://github.com/pmd/pmd/releases/download/pmd_releases%2F{pmd_ver}/pmd-dist-{pmd_ver}-bin.zip -o /tmp/pmd.zip && \
                 (cd $PMD_CACHE && jar xf /tmp/pmd.zip) && chmod +x $PMD_DIR/bin/pmd && rm -f /tmp/pmd.zip; \
               fi && \
               SOURCES=$(find . -path '*/src/main/java' -type d | tr '\\n' ',' | sed 's/,$//') && \
               if [ -z \"$SOURCES\" ]; then SOURCES=.; fi && \
               $PMD_DIR/bin/pmd check -d \"$SOURCES\" \
                 -R /workspace/pipelight-misc/pmd-ruleset.xml \
                 -f xml --no-cache \
                 -r /workspace/pipelight-misc/pmd-report/pmd-result.xml || true; \
             fi && \
             TOTAL=0 && \
             for f in /workspace/pipelight-misc/pmd-report/*.xml; do \
               [ -f \"$f\" ] || continue; \
               COUNT=$(grep -c '<violation' \"$f\" 2>/dev/null || echo 0); \
               if [ \"$COUNT\" -gt 0 ]; then echo \"  $(basename $f .xml): $COUNT violations\"; fi; \
               TOTAL=$((TOTAL + COUNT)); \
             done && \
             echo \"\" && echo \"PMD Total: $TOTAL violations\"; \
             else \
             echo 'PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - No pmd-ruleset.xml found in pipelight-misc/. LLM should search project for existing ruleset or coding guidelines to generate one.' >&2 && exit 1; \
             fi",
            cd = cd_prefix,
            pmd_ver = PMD_CLI_VERSION
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
        let violations = count_pattern(&output, &["PMD Total:"]);
        if violations > 0 {
            if let Some(line) = output.lines().find(|l| l.contains("PMD Total:")) {
                return line.trim().to_string();
            }
        }
        let violation_count = count_pattern(&output, &["violation", "Violation"]);
        if !success && violation_count == 0 { "pmd: failed".into() }
        else if violation_count > 0 { format!("pmd: {} violations (report only)", violation_count) }
        else { "pmd: no violations".into() }
    }
}
