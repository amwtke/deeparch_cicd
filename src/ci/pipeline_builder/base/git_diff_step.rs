use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::pipeline_builder::{StepConfig, StepDef};

/// Reports the three categories of local-but-not-yet-pushed changes:
/// unstaged working tree edits, staged (uncommitted) changes, and local
/// commits ahead of `@{upstream}`. Writes each list to its own file under
/// `pipelight-misc/git-diff-report/` and fires `GitDiffCommand` when any
/// category is non-empty so the LLM can render a grouped report.
///
/// Exits 0 (skipped) when:
/// - not a git repository
/// - working tree is clean AND no commits ahead of upstream
pub struct GitDiffStep;

impl GitDiffStep {
    pub fn new() -> Self {
        Self
    }
}

impl StepDef for GitDiffStep {
    fn config(&self) -> StepConfig {
        let script = r#"if ! git rev-parse --git-dir >/dev/null 2>&1; then echo 'git-diff: not a git repository — skipping'; exit 0; fi
REPORT_DIR=pipelight-misc/git-diff-report
mkdir -p "$REPORT_DIR"
UPSTREAM=$(git rev-parse --abbrev-ref --symbolic-full-name @{upstream} 2>/dev/null || true)
UNSTAGED=$(git diff --name-only --diff-filter=ACMR 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)
STAGED=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)
UNPUSHED=""
if [ -n "$UPSTREAM" ]; then
  UNPUSHED=$(git diff "$UPSTREAM"..HEAD --name-only --diff-filter=ACMR 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)
fi
printf '%s\n' "$UNSTAGED"  | sed '/^$/d' > "$REPORT_DIR/unstaged.txt"
printf '%s\n' "$STAGED"    | sed '/^$/d' > "$REPORT_DIR/staged.txt"
printf '%s\n' "$UNPUSHED"  | sed '/^$/d' > "$REPORT_DIR/unpushed.txt"
U=$(wc -l < "$REPORT_DIR/unstaged.txt" | tr -d ' ')
S=$(wc -l < "$REPORT_DIR/staged.txt"   | tr -d ' ')
P=$(wc -l < "$REPORT_DIR/unpushed.txt" | tr -d ' ')
TOTAL=$((U + S + P))
if [ "$TOTAL" -eq 0 ]; then echo 'git-diff: working tree clean and no unpushed commits — skipping'; exit 0; fi
echo "git-diff: $TOTAL change record(s) on current branch"
echo "  unstaged: $U file(s)"
echo "  staged: $S file(s)"
if [ -n "$UPSTREAM" ]; then echo "  unpushed (ahead of $UPSTREAM): $P file(s)"; else echo "  unpushed: n/a (no upstream configured)"; fi
exit 1"#;

        StepConfig {
            name: "git-diff".into(),
            local: true,
            commands: vec![script.into()],
            allow_failure: true,
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::GitDiffCommand).add(
            "git_diff_changes_found",
            ExceptionEntry {
                command: CallbackCommand::GitDiffCommand,
                max_retries: 0,
                context_paths: vec![
                    "pipelight-misc/git-diff-report/unstaged.txt".into(),
                    "pipelight-misc/git-diff-report/staged.txt".into(),
                    "pipelight-misc/git-diff-report/unpushed.txt".into(),
                ],
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, _stderr: &str) -> Option<String> {
        if stdout.contains("change record(s) on current branch") {
            Some("git_diff_changes_found".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("not a git repository") {
            return "git-diff: skipped (no git repo)".into();
        }
        if output.contains("working tree clean") {
            return "git-diff: skipped (tree clean)".into();
        }
        if let Some(line) = output
            .lines()
            .find(|l| l.contains("change record(s) on current branch"))
        {
            return line.trim().to_string();
        }
        if success {
            "git-diff: ok".into()
        } else {
            "git-diff: failed".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::callback::action::CallbackCommandAction;
    use crate::ci::callback::command::CallbackCommandRegistry;

    #[test]
    fn test_config_basic() {
        let step = GitDiffStep::new();
        let cfg = step.config();
        assert_eq!(cfg.name, "git-diff");
        assert!(cfg.local);
        assert!(cfg.allow_failure);
        assert_eq!(cfg.commands.len(), 1);
    }

    #[test]
    fn test_exception_mapping_default_is_git_diff_command() {
        let step = GitDiffStep::new();
        let of = step.exception_mapping().to_on_failure();
        assert_eq!(of.callback_command, CallbackCommand::GitDiffCommand);
        assert_eq!(of.max_retries, 0);
        assert_eq!(of.context_paths.len(), 3);
    }

    #[test]
    fn test_exception_mapping_changes_found_key() {
        let step = GitDiffStep::new();
        let match_fn = |code: i64, out: &str, err: &str| -> Option<String> {
            step.match_exception(code, out, err)
        };
        let resolved = step.exception_mapping().resolve(
            1,
            "git-diff: 3 change record(s) on current branch\n",
            "",
            Some(&match_fn),
        );
        assert_eq!(resolved.command, CallbackCommand::GitDiffCommand);
        assert_eq!(resolved.exception_key, "git_diff_changes_found");
    }

    #[test]
    fn test_registry_action_is_git_diff_report() {
        let registry = CallbackCommandRegistry::new();
        assert_eq!(
            registry.action_for(&CallbackCommand::GitDiffCommand),
            CallbackCommandAction::GitDiffReport
        );
    }

    #[test]
    fn test_report_not_a_repo() {
        let step = GitDiffStep::new();
        let r = step.output_report_str(true, "git-diff: not a git repository — skipping\n", "");
        assert_eq!(r, "git-diff: skipped (no git repo)");
    }

    #[test]
    fn test_report_clean() {
        let step = GitDiffStep::new();
        let r = step.output_report_str(
            true,
            "git-diff: working tree clean and no unpushed commits — skipping\n",
            "",
        );
        assert_eq!(r, "git-diff: skipped (tree clean)");
    }

    #[test]
    fn test_report_has_changes() {
        let step = GitDiffStep::new();
        let stdout = "git-diff: 5 change record(s) on current branch\n  unstaged: 2 file(s)\n  staged: 1 file(s)\n  unpushed (ahead of origin/main): 2 file(s)\n";
        let r = step.output_report_str(false, stdout, "");
        assert_eq!(r, "git-diff: 5 change record(s) on current branch");
    }
}
