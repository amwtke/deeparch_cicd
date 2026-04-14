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
        // PMD with two modes (kept in sync with gradle/pmd_step.rs):
        //
        //   Incremental mode (git repo present, PIPELIGHT_FULL unset):
        //     Scans source changes on the current branch that aren't yet pushed:
        //     - unstaged working tree edits
        //     - staged (uncommitted) changes
        //     - local commits ahead of @{upstream} (if upstream configured)
        //     If no changes → skip. Violations → auto_fix.
        //
        //   Full-scan mode (PIPELIGHT_FULL=1 from `pipelight run --full`, OR no git repo):
        //     Scans every src/main/{java,kotlin} dir, produces a report, and always
        //     exits 0 (report-only; no auto_fix, does not block the pipeline).
        //
        // Uses standalone PMD CLI (bypasses the Maven plugin to avoid its default-ruleset merge).
        let cmd = format!(
            "{cd}FULL_SCAN=${{PIPELIGHT_FULL:-0}} && \
             if [ \"$FULL_SCAN\" = \"1\" ]; then \
               echo 'PMD: --full requested — full scan (report-only)'; \
             elif ! git rev-parse --git-dir >/dev/null 2>&1; then \
               echo 'PMD: not a git repository — full scan (report-only)'; \
               FULL_SCAN=1; \
             fi && \
             if [ \"$FULL_SCAN\" = \"0\" ]; then \
               UPSTREAM=$(git rev-parse --abbrev-ref --symbolic-full-name @{{upstream}} 2>/dev/null || true) && \
               CHANGED_FILES=$( \
                 {{ git diff --relative --name-only --diff-filter=ACMR -- '*.java' '*.kt' 2>/dev/null; \
                    git diff --cached --relative --name-only --diff-filter=ACMR -- '*.java' '*.kt' 2>/dev/null; \
                    if [ -n \"$UPSTREAM\" ]; then \
                      git diff \"$UPSTREAM\"..HEAD --relative --name-only --diff-filter=ACMR -- '*.java' '*.kt' 2>/dev/null; \
                    fi; \
                 }} | sort -u | while read f; do [ -f \"$f\" ] && echo \"$f\"; done \
               ) && \
               if [ -z \"$CHANGED_FILES\" ]; then \
                 echo 'PMD: no changed source files on current branch — skipping'; \
                 exit 0; \
               fi && \
               echo \"PMD: scanning $(echo \"$CHANGED_FILES\" | wc -l | tr -d ' ') changed file(s) on current branch\"; \
             else \
               CHANGED_FILES=$(find . \\( -path '*/src/main/java' -o -path '*/src/main/kotlin' \\) -type d 2>/dev/null); \
               if [ -z \"$CHANGED_FILES\" ]; then CHANGED_FILES=.; fi; \
               echo \"PMD: full-scan sources:\"; echo \"$CHANGED_FILES\"; \
             fi && \
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
             mkdir -p $REPORT; \
             ( echo \"PMD Report Summary\"; echo \"==================\"; \
               echo \"Total violations: $TOTAL\"; echo \"\"; \
               echo \"By Rule:\"; \
               grep -o 'rule=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
                 | sed 's/rule=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"Top 10 Files:\"; \
               grep -o 'name=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
                 | sed 's/name=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             ) > $REPORT/pmd-summary.txt 2>/dev/null; \
             if [ \"$FULL_SCAN\" = \"1\" ]; then \
               echo \"PMD: full-scan report-only mode — report at /workspace/pipelight-misc/pmd-report/\"; \
               exit 0; \
             fi; \
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
            // Report-only: see gradle/pmd_step.rs for rationale.
            allow_failure: true,
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
            // PMD ran to completion; non-zero exit ⇒ violations found → pmd_print_command
            Some("pmd_violations".into())
        } else {
            // PMD did not run (git missing, network, IO) → fall through to default
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            return "pmd: ruleset not found (callback)".into();
        }
        if output.contains("no changed source files") {
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
