use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::pipeline_builder::{StepConfig, StepDef};

/// Reports the set of files that changed on the current branch as a
/// single deduplicated list under `pipelight-misc/git-diff-report/diff.txt`.
/// The list is a union of unstaged working-tree edits, staged changes,
/// untracked (new) files, and commits ahead of the branch-ahead base.
/// The base defaults to `@{upstream}` but can be switched via the
/// `base_ref` field (set from `--git-diff-from-remote-branch=<ref>`).
/// Fires `GitDiffCommand` when any category is non-empty so the LLM can
/// render a grouped report.
///
/// Exits 0 (skipped) when not a git repository, or the working tree is
/// clean AND there are no commits ahead of the configured base.
/// Exits 2 (runtime error) when a literal base ref was requested but
/// does not exist locally — the user must `git fetch` first.
pub struct GitDiffStep {
    /// `None` → use `@{upstream}` (original behavior).
    /// `Some("origin/main")` → use the given literal ref as branch-ahead base.
    base_ref: Option<String>,
}

impl GitDiffStep {
    pub fn new() -> Self {
        Self { base_ref: None }
    }

    /// The given ref (when `Some`) is interpolated directly into the shell
    /// script's `BASE="{ref}"` line — callers MUST pre-validate via
    /// `crate::cli::is_safe_ref` (or equivalent ASCII whitelist) to prevent
    /// shell injection. CLI is the only current caller and it validates.
    pub fn with_base_ref(base_ref: Option<String>) -> Self {
        if let Some(ref r) = base_ref {
            debug_assert!(
                !r.is_empty()
                    && r.chars().all(|c| c.is_ascii_alphanumeric() || "/_.-".contains(c)),
                "with_base_ref: unsafe ref '{r}' — must be ASCII alphanumeric + /_.-",
            );
        }
        Self { base_ref }
    }
}

impl StepDef for GitDiffStep {
    fn config(&self) -> StepConfig {
        // Prefix that switches branch-ahead BASE. Two variants:
        //   None        → compute BASE from git @{upstream}
        //   Some(ref)   → hardcode BASE to the given literal (e.g. "origin/main")
        let base_prefix = match &self.base_ref {
            None => r#"BASE=$(git rev-parse --abbrev-ref --symbolic-full-name @{upstream} 2>/dev/null || true)
BASE_LABEL="@{upstream}""#
                .to_string(),
            Some(r) => format!(
                r#"BASE="{r}"
BASE_LABEL="{r}""#,
                r = r
            ),
        };

        let body = r#"# pipelight:git-diff-step v2
if ! git rev-parse --git-dir >/dev/null 2>&1; then echo 'git-diff: not a git repository — skipping'; exit 0; fi
REPORT_DIR=pipelight-misc/git-diff-report
mkdir -p "$REPORT_DIR"
__BASE_PREFIX__

UNSTAGED=$(git diff --name-only --diff-filter=ACMR 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)
STAGED=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)
UNTRACKED=$(git ls-files --others --exclude-standard 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)

BRANCH_AHEAD=""
BRANCH_AHEAD_ERR=0
if [ -n "$BASE" ]; then
  if ! git rev-parse --verify "$BASE" >/dev/null 2>&1; then
    echo "git-diff: base ref '$BASE' not found — run 'git fetch' first" >&2
    BRANCH_AHEAD_ERR=1
  else
    BRANCH_AHEAD=$(git diff "$BASE"..HEAD --name-only --diff-filter=ACMR 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)
  fi
fi

U=$(printf '%s\n' "$UNSTAGED"     | sed '/^$/d' | wc -l | tr -d ' ')
S=$(printf '%s\n' "$STAGED"       | sed '/^$/d' | wc -l | tr -d ' ')
T=$(printf '%s\n' "$UNTRACKED"    | sed '/^$/d' | wc -l | tr -d ' ')
B=$(printf '%s\n' "$BRANCH_AHEAD" | sed '/^$/d' | wc -l | tr -d ' ')

{ printf '%s\n' "$UNSTAGED"; printf '%s\n' "$STAGED"; printf '%s\n' "$UNTRACKED"; printf '%s\n' "$BRANCH_AHEAD"; } \
  | sed '/^$/d' | sort -u \
  | while read f; do [ -f "$f" ] && echo "$f"; done \
  > "$REPORT_DIR/diff.txt"

TOTAL=$(wc -l < "$REPORT_DIR/diff.txt" | tr -d ' ')

if [ "$BRANCH_AHEAD_ERR" = "1" ]; then exit 2; fi

if [ "$TOTAL" -eq 0 ]; then echo 'git-diff: working tree clean and no branch-ahead commits — skipping'; exit 0; fi

__BASE_REF_SIDECAR__

echo "git-diff: $TOTAL unique file(s) changed on current branch"
echo "  unstaged: $U"
echo "  staged: $S"
echo "  untracked: $T"
if [ -n "$BASE" ]; then echo "  branch-ahead (vs $BASE_LABEL): $B"; else echo "  branch-ahead: n/a (no base ref configured)"; fi
exit 1"#;

        let script = body.replace("__BASE_PREFIX__", &base_prefix);
        let sidecar = match &self.base_ref {
            None => String::new(),
            Some(_) => r#"echo "$BASE" > "$REPORT_DIR/base-ref.txt""#.to_string(),
        };
        let script = script.replace("__BASE_REF_SIDECAR__", &sidecar);

        StepConfig {
            name: "git-diff".into(),
            local: true,
            commands: vec![script],
            allow_failure: true,
            ..Default::default()
        }
    }

    // exception_mapping / match_exception / output_report_str unchanged for now;
    // Task 4 + Task 5 update them.

    fn exception_mapping(&self) -> ExceptionMapping {
        let mut context_paths = vec!["pipelight-misc/git-diff-report/diff.txt".to_string()];
        if self.base_ref.is_some() {
            context_paths.push("pipelight-misc/git-diff-report/base-ref.txt".into());
        }
        ExceptionMapping::new(CallbackCommand::GitDiffCommand)
            .add(
                "git_diff_changes_found",
                ExceptionEntry {
                    command: CallbackCommand::GitDiffCommand,
                    max_retries: 0,
                    context_paths,
                },
            )
            .add(
                "git_diff_base_not_found",
                ExceptionEntry {
                    command: CallbackCommand::RuntimeError,
                    max_retries: 0,
                    context_paths: vec![],
                },
            )
    }

    fn match_exception(&self, exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        // Priority 1: explicit base ref missing (only when exit code is 2).
        if exit_code == 2 && stderr.contains("base ref") && stderr.contains("not found") {
            return Some("git_diff_base_not_found".into());
        }
        // Priority 2: normal "changes found" path.
        if stdout.contains("unique file(s) changed on current branch") {
            return Some("git_diff_changes_found".into());
        }
        None
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("not a git repository") {
            return "git-diff: skipped (no git repo)".into();
        }
        if output.contains("working tree clean") {
            return "git-diff: skipped (tree clean)".into();
        }
        if output.contains("base ref") && output.contains("not found") {
            return "git-diff: base ref not found".into();
        }
        if let Some(line) = output
            .lines()
            .find(|l| l.contains("unique file(s) changed on current branch"))
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
        assert_eq!(of.context_paths.len(), 1);
    }

    #[test]
    fn test_exception_mapping_changes_found_key() {
        let step = GitDiffStep::new();
        let match_fn = |code: i64, out: &str, err: &str| -> Option<String> {
            step.match_exception(code, out, err)
        };
        let resolved = step.exception_mapping().resolve(
            1,
            "git-diff: 3 unique file(s) changed on current branch\n",
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
            "git-diff: working tree clean and no branch-ahead commits — skipping\n",
            "",
        );
        assert_eq!(r, "git-diff: skipped (tree clean)");
    }

    #[test]
    fn test_report_has_changes() {
        let step = GitDiffStep::new();
        let stdout = "git-diff: 6 unique file(s) changed on current branch\n  unstaged: 2\n  staged: 1\n  untracked: 1\n  branch-ahead (vs origin/main): 2\n";
        let r = step.output_report_str(false, stdout, "");
        assert_eq!(r, "git-diff: 6 unique file(s) changed on current branch");
    }

    #[test]
    fn test_new_has_none_base_ref() {
        let step = GitDiffStep::new();
        assert_eq!(step.base_ref, None);
    }

    #[test]
    fn test_with_base_ref_some_stores_value() {
        let step = GitDiffStep::with_base_ref(Some("origin/main".into()));
        assert_eq!(step.base_ref.as_deref(), Some("origin/main"));
    }

    #[test]
    fn test_with_base_ref_none_equals_new() {
        let a = GitDiffStep::new();
        let b = GitDiffStep::with_base_ref(None);
        assert_eq!(a.base_ref, b.base_ref);
    }

    #[test]
    fn test_script_writes_single_diff_txt() {
        let step = GitDiffStep::new();
        let cmd = &step.config().commands[0];
        assert!(
            cmd.contains("> \"$REPORT_DIR/diff.txt\""),
            "script should redirect unified output to diff.txt; got:\n{cmd}"
        );
        assert!(
            !cmd.contains("unstaged.txt")
                && !cmd.contains("staged.txt")
                && !cmd.contains("untracked.txt")
                && !cmd.contains("unpushed.txt"),
            "script must not write legacy per-category files; got:\n{cmd}"
        );
    }

    #[test]
    fn test_new_variant_uses_upstream() {
        let step = GitDiffStep::new();
        let cmd = &step.config().commands[0];
        assert!(
            cmd.contains("@{upstream}"),
            "default variant must reference @{{upstream}}; got:\n{cmd}"
        );
        assert!(
            cmd.contains("BASE_LABEL=\"@{upstream}\""),
            "default variant must label BASE as @{{upstream}}; got:\n{cmd}"
        );
    }

    #[test]
    fn test_literal_variant_uses_given_ref() {
        let step = GitDiffStep::with_base_ref(Some("origin/main".into()));
        let cmd = &step.config().commands[0];
        assert!(
            cmd.contains("BASE=\"origin/main\""),
            "literal variant must hardcode the base ref; got:\n{cmd}"
        );
        assert!(
            cmd.contains("BASE_LABEL=\"origin/main\""),
            "literal variant must label BASE with the given ref; got:\n{cmd}"
        );
        assert!(
            !cmd.contains("@{upstream}"),
            "literal variant must NOT reference @{{upstream}}; got:\n{cmd}"
        );
    }

    #[test]
    fn test_script_sentinel_present() {
        let step = GitDiffStep::new();
        let cmd = &step.config().commands[0];
        assert!(
            cmd.contains("# pipelight:git-diff-step v2"),
            "script should carry sentinel comment for future version detection"
        );
    }

    #[test]
    fn test_script_still_detects_untracked_files() {
        let step = GitDiffStep::new();
        let cmd = &step.config().commands[0];
        assert!(
            cmd.contains("git ls-files --others --exclude-standard"),
            "script should still use git ls-files for untracked detection"
        );
    }

    #[test]
    fn test_exception_mapping_base_not_found_entry_exists() {
        let step = GitDiffStep::new();
        let mapping = step.exception_mapping();
        // Resolve an exception that matches "git_diff_base_not_found" via the
        // match_exception hook; assert the command is RuntimeError.
        let match_fn = |code: i64, out: &str, err: &str| -> Option<String> {
            step.match_exception(code, out, err)
        };
        let resolved = mapping.resolve(
            2,
            "",
            "git-diff: base ref 'origin/main' not found — run 'git fetch' first\n",
            Some(&match_fn),
        );
        assert_eq!(resolved.command, CallbackCommand::RuntimeError);
        assert_eq!(resolved.exception_key, "git_diff_base_not_found");
        assert_eq!(resolved.context_paths.len(), 0);
        assert_eq!(resolved.max_retries, 0);
    }

    #[test]
    fn test_match_exception_base_not_found_priority() {
        let step = GitDiffStep::new();
        // Even if stdout also has a "unique file(s) changed" line, an exit code
        // of 2 with the stderr marker must win and return base_not_found.
        let out = "git-diff: 1 unique file(s) changed on current branch\n";
        let err = "git-diff: base ref 'origin/foo' not found — run 'git fetch' first\n";
        let key = step.match_exception(2, out, err);
        assert_eq!(key.as_deref(), Some("git_diff_base_not_found"));
    }

    #[test]
    fn test_match_exception_changes_found_on_exit_1() {
        let step = GitDiffStep::new();
        let out = "git-diff: 3 unique file(s) changed on current branch\n";
        let key = step.match_exception(1, out, "");
        assert_eq!(key.as_deref(), Some("git_diff_changes_found"));
    }

    #[test]
    fn test_match_exception_no_match_on_clean() {
        let step = GitDiffStep::new();
        let out = "git-diff: working tree clean and no branch-ahead commits — skipping\n";
        let key = step.match_exception(0, out, "");
        assert_eq!(key, None);
    }

    #[test]
    fn test_registry_action_for_base_not_found_is_runtime_error() {
        let registry = CallbackCommandRegistry::new();
        assert_eq!(
            registry.action_for(&CallbackCommand::RuntimeError),
            CallbackCommandAction::RuntimeError
        );
    }

    #[test]
    fn test_match_exception_no_longer_matches_legacy_string() {
        let step = GitDiffStep::new();
        // The legacy "change record(s)" phrasing must NOT match anymore —
        // no real script produces it, and matcher must only key off the new text.
        let out = "git-diff: 3 change record(s) on current branch\n";
        let key = step.match_exception(1, out, "");
        assert_eq!(key, None, "legacy marker should no longer match");
    }

    #[test]
    fn test_output_report_str_base_not_found() {
        let step = GitDiffStep::new();
        let r = step.output_report_str(
            false,
            "",
            "git-diff: base ref 'origin/foo' not found — run 'git fetch' first\n",
        );
        assert_eq!(r, "git-diff: base ref not found");
    }

    #[test]
    fn test_script_writes_base_ref_file_when_some() {
        let step = GitDiffStep::with_base_ref(Some("origin/main".into()));
        let cmd = &step.config().commands[0];
        assert!(
            cmd.contains(r#"echo "$BASE" > "$REPORT_DIR/base-ref.txt""#),
            "literal variant must write base-ref.txt; got:\n{cmd}"
        );
    }

    #[test]
    fn test_script_does_not_write_base_ref_file_when_none() {
        let step = GitDiffStep::new();
        let cmd = &step.config().commands[0];
        assert!(
            !cmd.contains("base-ref.txt"),
            "default variant must NOT mention base-ref.txt; got:\n{cmd}"
        );
    }

    #[test]
    fn test_base_ref_file_written_after_branch_ahead_err_guard() {
        let step = GitDiffStep::with_base_ref(Some("origin/main".into()));
        let cmd = &step.config().commands[0];
        let guard_idx = cmd
            .find(r#"if [ "$BRANCH_AHEAD_ERR" = "1" ]; then exit 2; fi"#)
            .expect("BRANCH_AHEAD_ERR guard must be present");
        let sidecar_idx = cmd
            .find(r#"echo "$BASE" > "$REPORT_DIR/base-ref.txt""#)
            .expect("sidecar write must be present for Some(ref)");
        assert!(
            sidecar_idx > guard_idx,
            "sidecar write (idx={sidecar_idx}) must come AFTER BRANCH_AHEAD_ERR guard (idx={guard_idx})"
        );
    }

    #[test]
    fn test_context_paths_one_path_when_none() {
        let step = GitDiffStep::new();
        let of = step.exception_mapping().to_on_failure();
        assert_eq!(
            of.context_paths,
            vec!["pipelight-misc/git-diff-report/diff.txt"],
            "None variant must carry only diff.txt"
        );
    }

    #[test]
    fn test_context_paths_includes_base_ref_file_when_some() {
        let step = GitDiffStep::with_base_ref(Some("origin/main".into()));
        let match_fn = |code: i64, out: &str, err: &str| -> Option<String> {
            step.match_exception(code, out, err)
        };
        let resolved = step.exception_mapping().resolve(
            1,
            "git-diff: 3 unique file(s) changed on current branch\n",
            "",
            Some(&match_fn),
        );
        assert_eq!(resolved.exception_key, "git_diff_changes_found");
        assert_eq!(
            resolved.context_paths,
            vec![
                "pipelight-misc/git-diff-report/diff.txt".to_string(),
                "pipelight-misc/git-diff-report/base-ref.txt".to_string(),
            ],
            "Some variant must carry both diff.txt and base-ref.txt in that order"
        );
    }
}
