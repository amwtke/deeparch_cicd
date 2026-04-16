use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, git_changed_files_snippet, StepConfig, StepDef};

/// PMD version used for standalone CLI.
const PMD_CLI_VERSION: &str = "7.9.0";

/// Incremental PMD step (tag = "non-full").
///
/// Scans only source changes on the current branch that aren't yet pushed.
/// Skips when there's no git repo or no pending changes. Violations trigger
/// an `auto_fix` callback. Full-repo scans live in `pmd_full_step`.
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
            "{cd}if ! git rev-parse --git-dir >/dev/null 2>&1; then \
               echo 'PMD: not a git repository — skipping (use pmd_full for full scan)'; \
               exit 0; \
             fi && \
             {changed_files} && \
             if [ -z \"$CHANGED_FILES\" ]; then \
               echo 'PMD: no changed source files on current branch — skipping'; \
               exit 0; \
             fi && \
             echo \"PMD: scanning $(echo \"$CHANGED_FILES\" | wc -l | tr -d ' ') changed file(s) on current branch\" && \
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
             ( echo \"PMD Report Summary\"; echo \"==================\"; \
               echo \"Total violations: $TOTAL\"; echo \"\"; \
               echo \"By Rule:\"; \
               grep -o 'rule=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
                 | sed 's/rule=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"Top 10 Files:\"; \
               grep -o 'name=\"[^\"]*\"' $REPORT/pmd-result.xml 2>/dev/null \
                 | sed 's/name=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             ) > $REPORT/pmd-summary.txt 2>/dev/null; \
             if [ \"$TOTAL\" -gt 0 ]; then exit 1; fi; \
             exit $PMD_RC",
            cd = cd_prefix,
            pmd_ver = PMD_CLI_VERSION,
            changed_files = git_changed_files_snippet(&["*.java", "*.kt"], self.subdir.as_deref())
        );
        StepConfig {
            name: "pmd".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            allow_failure: false,
            tag: "non-full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError)
            .add(
                "pmd_violations",
                ExceptionEntry {
                    command: CallbackCommand::AutoFix,
                    max_retries: 9,
                    context_paths: vec![
                        "pipelight-misc/pmd-report/pmd-result.xml".into(),
                        "pipelight-misc/pmd-report/pmd-summary.txt".into(),
                        "pipelight-misc/git-diff-report/staged.txt".into(),
                        "pipelight-misc/git-diff-report/unstaged.txt".into(),
                        "pipelight-misc/git-diff-report/untracked.txt".into(),
                        "pipelight-misc/git-diff-report/unpushed.txt".into(),
                    ],
                },
            )
            .add(
                "ruleset_not_found",
                ExceptionEntry {
                    command: CallbackCommand::AutoGenPmdRuleset,
                    max_retries: 9,
                    context_paths: self.source_paths.clone(),
                },
            )
            .add(
                "ruleset_invalid",
                ExceptionEntry {
                    command: CallbackCommand::AutoGenPmdRuleset,
                    max_retries: 9,
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
            return "pmd: ruleset not found (callback)".into();
        }
        if output.contains("not a git repository") {
            return "pmd: skipped (no git repo)".into();
        }
        if output.contains("no changed source files") {
            return "pmd: skipped (no changed files)".into();
        }
        if let Some(line) = output.lines().find(|l| l.contains("PMD Total:")) {
            return line.trim().to_string();
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
