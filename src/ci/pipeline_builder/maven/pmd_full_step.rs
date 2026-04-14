use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

/// PMD version used for standalone CLI.
const PMD_CLI_VERSION: &str = "7.9.0";

/// Full-repo PMD scan (tag = "full").
///
/// Activated by `--full-report-only`. Always scans every
/// `src/main/{java,kotlin}` dir and produces a report. Never auto-fixes —
/// violations are surfaced via `pmd_print_command` so the LLM prints the
/// findings; pipeline is not blocked (`allow_failure: true`).
pub struct PmdFullStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
}

impl PmdFullStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for PmdFullStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{cd}SOURCE_DIRS=$(find . \\( -path '*/src/main/java' -o -path '*/src/main/kotlin' \\) -type d 2>/dev/null); \
             if [ -z \"$SOURCE_DIRS\" ]; then SOURCE_DIRS=.; fi && \
             echo 'PMD (full): scanning sources:'; echo \"$SOURCE_DIRS\" && \
             if [ ! -f /workspace/pipelight-misc/pmd-ruleset.xml ]; then \
               echo 'PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - No pmd-ruleset.xml found in pipelight-misc/. LLM should search project for existing ruleset or coding guidelines to generate one. IMPORTANT: Use PMD {pmd_ver} rule names (not PMD 6.x). Verify rule names exist in PMD 7 before writing the ruleset.' >&2 && exit 1; \
             fi && \
             PMD_CACHE=$HOME/.pipelight/cache && \
             PMD_DIR=$PMD_CACHE/pmd-bin-{pmd_ver} && \
             if [ ! -f $PMD_DIR/bin/pmd ]; then \
               mkdir -p $PMD_CACHE && \
               curl -sL https://github.com/pmd/pmd/releases/download/pmd_releases%2F{pmd_ver}/pmd-dist-{pmd_ver}-bin.zip -o /tmp/pmd.zip && \
               (cd $PMD_CACHE && jar xf /tmp/pmd.zip) && chmod +x $PMD_DIR/bin/pmd && rm -f /tmp/pmd.zip; \
             fi && \
             SOURCES=$(echo \"$SOURCE_DIRS\" | tr '\\n' ',' | sed 's/,$//') && \
             mkdir -p /workspace/pipelight-misc/pmd-report && \
             $PMD_DIR/bin/pmd check -d \"$SOURCES\" \
               -R /workspace/pipelight-misc/pmd-ruleset.xml \
               -f xml --no-cache \
               -r /workspace/pipelight-misc/pmd-report/pmd-result.xml \
               2>/tmp/pmd-stderr.log; \
             if grep -q 'Cannot load ruleset\\|Unable to find referenced rule' /tmp/pmd-stderr.log 2>/dev/null; then \
               echo 'ERROR: PMD ruleset has invalid rules. Details:' >&2; \
               grep 'Unable to find referenced rule\\|Cannot load ruleset\\|XML validation error' /tmp/pmd-stderr.log >&2; \
               echo 'PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - pmd-ruleset.xml contains invalid rule references for PMD {pmd_ver}. LLM must regenerate with correct PMD 7.x rule names.' >&2; \
               exit 1; \
             fi; \
             REPORT=/workspace/pipelight-misc/pmd-report; \
             TOTAL=$(grep -c '<violation' $REPORT/pmd-result.xml 2>/dev/null); \
             TOTAL=${{TOTAL:-0}}; \
             echo \"\"; echo \"PMD Total: $TOTAL violations\"; \
             echo \"\"; echo \"=== Violations by Rule ===\"; \
             grep -o 'rule=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
               | sed 's/rule=\"//;s/\"//' | sort | uniq -c | sort -rn; \
             echo \"\"; echo \"=== Top 10 Files ===\"; \
             grep -o 'name=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
               | sed 's/name=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             if [ \"$TOTAL\" -gt 0 ]; then \
               $PMD_DIR/bin/pmd check -d \"$SOURCES\" \
                 -R /workspace/pipelight-misc/pmd-ruleset.xml \
                 -f html --no-cache --no-progress \
                 -r $REPORT/pmd-result.html 2>/dev/null || true; \
             fi; \
             ( echo \"PMD Report Summary\"; echo \"==================\"; \
               echo \"Total violations: $TOTAL\"; echo \"\"; \
               echo \"By Rule:\"; \
               grep -o 'rule=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
                 | sed 's/rule=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"Top 10 Files:\"; \
               grep -o 'name=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
                 | sed 's/name=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             ) > $REPORT/pmd-summary.txt 2>/dev/null; \
             echo \"PMD (full): report at /workspace/pipelight-misc/pmd-report/\"; \
             exit 0",
            cd = cd_prefix,
            pmd_ver = PMD_CLI_VERSION,
        );
        StepConfig {
            name: "pmd_full".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            allow_failure: true,
            active: false,
            tag: "full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError)
            .add(
                "pmd_violations",
                ExceptionEntry {
                    command: CallbackCommand::PmdPrintCommand,
                    max_retries: 0,
                    context_paths: vec![
                        "pipelight-misc/pmd-report/pmd-result.xml".into(),
                        "pipelight-misc/pmd-report/pmd-summary.txt".into(),
                    ],
                },
            )
            .add(
                "ruleset_not_found",
                ExceptionEntry {
                    command: CallbackCommand::AutoGenPmdRuleset,
                    max_retries: 2,
                    context_paths: self.source_paths.clone(),
                },
            )
            .add(
                "ruleset_invalid",
                ExceptionEntry {
                    command: CallbackCommand::AutoGenPmdRuleset,
                    max_retries: 2,
                    context_paths: self.source_paths.clone(),
                },
            )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        if stderr.contains("Cannot load ruleset")
            || stderr.contains("Unable to find referenced rule")
        {
            Some("ruleset_invalid".into())
        } else if stderr.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            Some("ruleset_not_found".into())
        } else if stdout.contains("PMD Total:") {
            Some("pmd_violations".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            return "pmd_full: ruleset not found (callback)".into();
        }
        if let Some(line) = output.lines().find(|l| l.contains("PMD Total:")) {
            return line.trim().to_string();
        }
        let violation_count = count_pattern(&output, &["violation", "Violation"]);
        if !success && violation_count == 0 {
            "pmd_full: failed".into()
        } else if violation_count > 0 {
            format!("pmd_full: {} violations (report only)", violation_count)
        } else {
            "pmd_full: no violations".into()
        }
    }
}
