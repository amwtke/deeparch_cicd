use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

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
        // If found: use standalone PMD CLI to scan with ONLY our ruleset (Maven plugin
        //   always merges its default ruleset, so we bypass it entirely).
        // If not found: emit callback marker and exit 1 so the LLM can search/generate a ruleset.
        let cmd = format!(
            "{cd}if [ -f /workspace/pipelight-misc/pmd-ruleset.xml ]; then \
             PMD_VER=7.9.0 && \
             PMD_CACHE=$HOME/.pipelight/cache && \
             PMD_DIR=$PMD_CACHE/pmd-bin-$PMD_VER && \
             if [ ! -f $PMD_DIR/bin/pmd ]; then \
               echo 'Downloading PMD CLI...' && \
               mkdir -p $PMD_CACHE && \
               curl -sL https://github.com/pmd/pmd/releases/download/pmd_releases%2F$PMD_VER/pmd-dist-$PMD_VER-bin.zip -o /tmp/pmd.zip && \
               (cd $PMD_CACHE && jar xf /tmp/pmd.zip) && chmod +x $PMD_DIR/bin/pmd && rm -f /tmp/pmd.zip; \
             fi && \
             SOURCES=$(find . -path '*/src/main/java' -type d | tr '\\n' ',' | sed 's/,$//') && \
             if [ -z \"$SOURCES\" ]; then SOURCES=.; fi && \
             mkdir -p /workspace/pipelight-misc/pmd-report && \
             $PMD_DIR/bin/pmd check -d \"$SOURCES\" \
               -R /workspace/pipelight-misc/pmd-ruleset.xml \
               -f xml --no-cache \
               -r /workspace/pipelight-misc/pmd-report/pmd-result.xml \
               2>/tmp/pmd-stderr.log; \
             if grep -q 'Cannot load ruleset\\|Unable to find referenced rule' /tmp/pmd-stderr.log 2>/dev/null; then \
               echo 'ERROR: PMD ruleset has invalid rules. Details:' >&2; \
               grep 'Unable to find referenced rule\\|Cannot load ruleset\\|XML validation error' /tmp/pmd-stderr.log >&2; \
               echo 'PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - pmd-ruleset.xml contains invalid rule references for PMD 7.9.0. LLM must regenerate with correct PMD 7.x rule names.' >&2; \
               exit 1; \
             fi; \
             TOTAL=$(grep -c '<violation' /workspace/pipelight-misc/pmd-report/pmd-result.xml 2>/dev/null || echo 0); \
             REPORT=/workspace/pipelight-misc/pmd-report; \
             echo \"\"; echo \"PMD Total: $TOTAL violations\"; \
             echo \"\"; echo \"=== Violations by Rule ===\"; \
             grep -o 'rule=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
               | sed 's/rule=\"//;s/\"//' | sort | uniq -c | sort -rn; \
             echo \"\"; echo \"=== Top 10 Files ===\"; \
             grep -o 'name=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
               | sed 's/name=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             $PMD_DIR/bin/pmd check -d \"$SOURCES\" \
               -R /workspace/pipelight-misc/pmd-ruleset.xml \
               -f html --no-cache --no-progress \
               -r $REPORT/pmd-result.html 2>/dev/null || true; \
             ( echo \"PMD Report Summary\"; echo \"==================\"; \
               echo \"Total violations: $TOTAL\"; echo \"\"; \
               echo \"By Rule:\"; \
               grep -o 'rule=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
                 | sed 's/rule=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"Top 10 Files:\"; \
               grep -o 'name=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
                 | sed 's/name=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             ) > $REPORT/pmd-summary.txt 2>/dev/null; \
             else \
             echo 'PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - No pmd-ruleset.xml found in pipelight-misc/. LLM should search project for existing ruleset or coding guidelines to generate one. IMPORTANT: Use PMD 7.9.0 rule names (not PMD 6.x). Verify rule names exist in PMD 7 before writing the ruleset.' >&2 && exit 1; \
             fi",
            cd = cd_prefix
        );
        StepConfig {
            name: "pmd".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
            .add("ruleset_not_found", ExceptionEntry {
                command: CallbackCommand::AutoGenPmdRuleset,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            })
            .add("ruleset_invalid", ExceptionEntry {
                command: CallbackCommand::AutoGenPmdRuleset,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, stderr: &str) -> Option<String> {
        if stderr.contains("Cannot load ruleset") || stderr.contains("Unable to find referenced rule") {
            Some("ruleset_invalid".into())
        } else if stderr.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            Some("ruleset_not_found".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            return "pmd: ruleset not found (callback)".into();
        }
        let violations = count_pattern(&output, &["PMD Total:"]);
        if violations > 0 {
            // Extract the "PMD Total: N violations" line from output
            if let Some(line) = output.lines().find(|l| l.contains("PMD Total:")) {
                return line.trim().to_string();
            }
        }
        let violation_count = count_pattern(&output, &["violation", "Violation"]);
        if !success && violation_count == 0 {
            "pmd: failed".into()
        } else if violation_count > 0 {
            format!("pmd: {} violations (report only)", violation_count)
        } else {
            "pmd: no violations".into()
        }
    }
}
