use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

/// PMD version used for standalone CLI.
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
        // Incremental PMD: only scan staged + unpushed-commit Java files.
        // Always uses standalone PMD CLI for file-level targeting.
        //
        // Flow:
        //   1. Collect changed .java files (git staged + origin/main..HEAD)
        //   2. If none → skip with success
        //   3. Check ruleset exists → if not, callback auto_gen_pmd_ruleset
        //   4. Ensure standalone PMD CLI cached
        //   5. Run PMD on changed files only
        //   6. Check for ruleset errors → callback auto_gen_pmd_ruleset
        //   7. Report violations (non-zero exit if any found → triggers auto_fix)
        let cmd = format!(
            "{cd}CHANGED_FILES=$( \
               {{ git diff --cached --name-only --diff-filter=ACMR -- '*.java' 2>/dev/null; \
                  git diff origin/main..HEAD --name-only --diff-filter=ACMR -- '*.java' 2>/dev/null; \
               }} | sort -u | while read f; do [ -f \"$f\" ] && echo \"$f\"; done \
             ) && \
             if [ -z \"$CHANGED_FILES\" ]; then \
               echo 'PMD: no changed Java files — skipping'; \
               exit 0; \
             fi && \
             echo \"PMD: scanning $(echo \"$CHANGED_FILES\" | wc -l | tr -d ' ') changed file(s)\" && \
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
             SOURCES=$(echo \"$CHANGED_FILES\" | tr '\\n' ',' | sed 's/,$//') && \
             mkdir -p /workspace/pipelight-misc/pmd-report && \
             $PMD_DIR/bin/pmd check -d \"$SOURCES\" \
               -R /workspace/pipelight-misc/pmd-ruleset.xml \
               -f xml --no-cache \
               -r /workspace/pipelight-misc/pmd-report/pmd-result.xml \
               2>/tmp/pmd-stderr.log; \
             PMD_RC=$?; \
             if grep -q 'Cannot load ruleset\\|Unable to find referenced rule' /tmp/pmd-stderr.log 2>/dev/null; then \
               echo 'ERROR: PMD ruleset has invalid rules. Details:' >&2; \
               grep 'Unable to find referenced rule\\|Cannot load ruleset\\|XML validation error' /tmp/pmd-stderr.log >&2; \
               echo 'PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - pmd-ruleset.xml contains invalid rule references for PMD {pmd_ver}. LLM must regenerate with correct PMD 7.x rule names.' >&2; \
               exit 1; \
             fi; \
             REPORT=/workspace/pipelight-misc/pmd-report; \
             TOTAL=0; \
             for f in $REPORT/*.xml; do \
               [ -f \"$f\" ] || continue; \
               COUNT=$(grep -c '<violation' \"$f\" 2>/dev/null || echo 0); \
               if [ \"$COUNT\" -gt 0 ]; then echo \"  $(basename $f .xml): $COUNT violations\"; fi; \
               TOTAL=$((TOTAL + COUNT)); \
             done; \
             echo \"\"; echo \"PMD Total: $TOTAL violations\"; \
             echo \"Scanned files: $(echo \"$CHANGED_FILES\" | wc -l | tr -d ' ')\"; \
             echo \"\"; echo \"=== Violations by Rule ===\"; \
             for f in $REPORT/*.xml; do \
               [ -f \"$f\" ] || continue; \
               grep -o 'rule=\"[^\"]*\"' \"$f\" 2>/dev/null; \
             done | sed 's/rule=\"//;s/\"//' | sort | uniq -c | sort -rn; \
             echo \"\"; echo \"=== Top 10 Files ===\"; \
             for f in $REPORT/*.xml; do \
               [ -f \"$f\" ] || continue; \
               grep -o 'name=\"[^\"]*\"' \"$f\" 2>/dev/null; \
             done | sed 's/name=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             $PMD_DIR/bin/pmd check -d \"$SOURCES\" \
               -R /workspace/pipelight-misc/pmd-ruleset.xml \
               -f html --no-cache --no-progress \
               -r $REPORT/pmd-result.html 2>/dev/null || true; \
             ( echo \"PMD Report Summary\"; echo \"==================\"; \
               echo \"Total violations: $TOTAL\"; echo \"\"; \
               echo \"Scanned files:\"; echo \"$CHANGED_FILES\"; echo \"\"; \
               echo \"By Rule:\"; \
               for f in $REPORT/*.xml; do \
                 [ -f \"$f\" ] || continue; \
                 grep -o 'rule=\"[^\"]*\"' \"$f\" 2>/dev/null; \
               done | sed 's/rule=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"Top 10 Files:\"; \
               for f in $REPORT/*.xml; do \
                 [ -f \"$f\" ] || continue; \
                 grep -o 'name=\"[^\"]*\"' \"$f\" 2>/dev/null; \
               done | sed 's/name=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             ) > $REPORT/pmd-summary.txt 2>/dev/null; \
             if [ \"$TOTAL\" -gt 0 ]; then exit 1; fi; \
             exit $PMD_RC",
            cd = cd_prefix,
            pmd_ver = PMD_CLI_VERSION
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
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add(
                "pmd_violation",
                ExceptionEntry {
                    command: CallbackCommand::AutoFix,
                    max_retries: 3,
                    context_paths: self.source_paths.clone(),
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

    fn match_exception(&self, _exit_code: i64, _stdout: &str, stderr: &str) -> Option<String> {
        if stderr.contains("Cannot load ruleset")
            || stderr.contains("Unable to find referenced rule")
        {
            Some("ruleset_invalid".into())
        } else if stderr.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            Some("ruleset_not_found".into())
        } else {
            // Any other failure (PMD found violations) → auto_fix
            Some("pmd_violation".into())
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            return "pmd: ruleset not found (callback)".into();
        }
        if output.contains("no changed Java files") {
            return "pmd: skipped (no changed files)".into();
        }
        let violations = count_pattern(&output, &["PMD Total:"]);
        if violations > 0 {
            if let Some(line) = output.lines().find(|l| l.contains("PMD Total:")) {
                return line.trim().to_string();
            }
        }
        let violation_count = count_pattern(&output, &["violation", "Violation"]);
        if !success && violation_count == 0 {
            "pmd: failed".into()
        } else if violation_count > 0 {
            format!("pmd: {} violations", violation_count)
        } else {
            "pmd: no violations".into()
        }
    }
}
