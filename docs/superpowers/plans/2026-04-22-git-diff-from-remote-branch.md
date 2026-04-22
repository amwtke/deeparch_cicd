# --git-diff-from-remote-branch Flag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `--git-diff-from-remote-branch=<ref>` CLI flag that lets users pick a remote branch (e.g. `origin/main`) as the base for the incremental code-quality scan scope, plus refactor `git-diff` step output from four per-category files into a single deduplicated `diff.txt`.

**Architecture:** The `git-diff` step script is rewritten to support two base-ref variants (default `@{upstream}`, or an explicit literal like `origin/main`). When the new flag is set, `cmd_run` finds the `git-diff` step in the in-memory pipeline and overwrites its `commands[0]` with the literal variant. The value is persisted into `RunState` so retries inherit it. Downstream PMD / SpotBugs / JaCoCo steps continue to work unchanged—they already read via the shared helper `git_changed_files_snippet`, which is simplified to read the single `diff.txt`.

**Tech Stack:** Rust, clap, serde, tokio. Tests use `cargo test -p pipelight`.

> **Note on `cargo test` invocations:**
> - `pipelight` is a binary-only crate. Do NOT pass `--lib` to `cargo test` — it errors with "no library targets found". Use `cargo test -p pipelight <filter>` directly.
> - `cargo test` accepts only ONE positional filter. Where this plan lists multiple filters on a single line (e.g. `test_a test_b test_c`), collapse them to the shared parent-module prefix (e.g. `ci::pipeline_builder::base::git_diff_step`) which catches all listed tests in one run, or invoke `cargo test` once per filter.

**Spec reference:** `docs/superpowers/specs/2026-04-21-git-diff-from-remote-branch-design.md`

---

## File Structure

**Created:** none.

**Modified:**

- `src/run_state/mod.rs` — add `git_diff_base: Option<String>` field to `RunState`
- `src/ci/pipeline_builder/base/git_diff_step.rs` — refactor struct, script, exception mapping, output_report_str, tests
- `src/ci/pipeline_builder/mod.rs` — simplify `git_changed_files_snippet` to read single `diff.txt`
- `src/cli/mod.rs` — add `git_diff_from_remote_branch` flag on `Run` and `Retry` subcommands; thread through `cmd_run` / `cmd_retry`
- `global-skills/pipelight-run/SKILL.md` — document the new flag and the single `diff.txt` output

**Boundaries:**

- `RunState` stays the sole owner of persisted flag values across run/retry.
- `GitDiffStep` owns *only* the shell script text and exception mapping; flag plumbing lives in `cli`.
- `git_changed_files_snippet` remains the single gateway to `diff.txt` for downstream consumers.

---

## Task 1: Add `git_diff_base` field to `RunState`

**Files:**
- Modify: `src/run_state/mod.rs:65-88`

- [ ] **Step 1: Write the failing test**

Append to the existing test module in `src/run_state/mod.rs` (find the `#[cfg(test)] mod tests { ... }` block and add inside):

```rust
#[test]
fn test_run_state_git_diff_base_default_none() {
    let state = RunState::new("r1", "p1");
    assert_eq!(state.git_diff_base, None);
}

#[test]
fn test_run_state_git_diff_base_roundtrip() {
    let mut state = RunState::new("r1", "p1");
    state.git_diff_base = Some("origin/main".into());
    let json = serde_json::to_string(&state).unwrap();
    let restored: RunState = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.git_diff_base, Some("origin/main".into()));
}

#[test]
fn test_run_state_git_diff_base_legacy_deserialize_missing_field() {
    // JSON produced by an older pipelight that didn't know about this field
    // must still deserialize, with git_diff_base defaulting to None.
    let legacy = r#"{
        "run_id":"r1","pipeline":"p1","status":"running",
        "duration_ms":null,"steps":[],"full_report_only":false
    }"#;
    let state: RunState = serde_json::from_str(legacy).unwrap();
    assert_eq!(state.git_diff_base, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight run_state::tests::test_run_state_git_diff_base -- --nocapture`
Expected: 3 compile errors — `no field 'git_diff_base' on type RunState`.

- [ ] **Step 3: Add the field and default**

Edit `src/run_state/mod.rs`. In the `RunState` struct (around line 65-76), add the field directly after `full_report_only`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    pub pipeline: String,
    pub status: PipelineStatus,
    pub duration_ms: Option<u64>,
    pub steps: Vec<StepState>,
    /// Full-scan report-only mode flag set by `pipelight run --full-report-only`;
    /// persisted so retries inherit the same scan semantics.
    #[serde(default, alias = "full")]
    pub full_report_only: bool,
    /// Remote ref used as the branch-ahead base for `git-diff` (e.g. `origin/main`).
    /// `None` means use `@{upstream}` (original behavior). Set by
    /// `pipelight run --git-diff-from-remote-branch`; persisted so retries inherit it.
    #[serde(default)]
    pub git_diff_base: Option<String>,
}
```

And in `RunState::new` (around line 79-88), initialize it:

```rust
impl RunState {
    pub fn new(run_id: &str, pipeline_name: &str) -> Self {
        Self {
            run_id: run_id.to_string(),
            pipeline: pipeline_name.to_string(),
            status: PipelineStatus::Running,
            duration_ms: None,
            steps: Vec::new(),
            full_report_only: false,
            git_diff_base: None,
        }
    }
    // ... rest unchanged
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight run_state::tests::test_run_state_git_diff_base -- --nocapture`
Expected: PASS (3 tests).

- [ ] **Step 5: Run the full run_state suite to make sure nothing regressed**

Run: `cargo test -p pipelight run_state`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/run_state/mod.rs
git commit -m "feat(run_state): add git_diff_base field for branch-ahead base ref persistence"
```

---

## Task 2: Refactor `GitDiffStep` struct to hold `base_ref`

**Files:**
- Modify: `src/ci/pipeline_builder/base/git_diff_step.rs:15-21`

- [ ] **Step 1: Write failing tests**

Append to the test module at the bottom of `src/ci/pipeline_builder/base/git_diff_step.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_new_has_none_base_ref`
Expected: FAIL — compile error `no field 'base_ref'`.

- [ ] **Step 3: Modify struct and constructors**

In `src/ci/pipeline_builder/base/git_diff_step.rs` lines 15-21, replace:

```rust
pub struct GitDiffStep;

impl GitDiffStep {
    pub fn new() -> Self {
        Self
    }
}
```

with:

```rust
pub struct GitDiffStep {
    /// `None` → use `@{upstream}` (original behavior).
    /// `Some("origin/main")` → use the given literal ref as branch-ahead base.
    base_ref: Option<String>,
}

impl GitDiffStep {
    pub fn new() -> Self {
        Self { base_ref: None }
    }

    pub fn with_base_ref(base_ref: Option<String>) -> Self {
        Self { base_ref }
    }
}
```

- [ ] **Step 4: Run the three new tests**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_new_has_none_base_ref ci::pipeline_builder::base::git_diff_step::tests::test_with_base_ref_some_stores_value ci::pipeline_builder::base::git_diff_step::tests::test_with_base_ref_none_equals_new`
Expected: PASS (3 tests).

- [ ] **Step 5: Run existing git_diff_step suite to spot anything that still compiles**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step`
Expected: existing tests still pass (script body hasn't been rewritten yet). If any fail due to implicit `Self { }` vs `Self { base_ref: None }` moves, they'll be fixed in Task 3.

- [ ] **Step 6: Do NOT commit yet** — Task 3 rewrites the script and changes exception mapping; commit once the full refactor compiles cleanly.

---

## Task 3: Rewrite `GitDiffStep` script to produce single `diff.txt`

**Files:**
- Modify: `src/ci/pipeline_builder/base/git_diff_step.rs:23-60`

- [ ] **Step 1: Write failing tests**

Append to the test module:

```rust
#[test]
fn test_script_writes_single_diff_txt() {
    let step = GitDiffStep::new();
    let cmd = &step.config().commands[0];
    assert!(
        cmd.contains("> \"$REPORT_DIR/diff.txt\""),
        "script should redirect unified output to diff.txt; got:\n{cmd}"
    );
    assert!(
        !cmd.contains("unstaged.txt") && !cmd.contains("staged.txt")
            && !cmd.contains("untracked.txt") && !cmd.contains("unpushed.txt"),
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_script_writes_single_diff_txt ci::pipeline_builder::base::git_diff_step::tests::test_new_variant_uses_upstream ci::pipeline_builder::base::git_diff_step::tests::test_literal_variant_uses_given_ref ci::pipeline_builder::base::git_diff_step::tests::test_script_sentinel_present`
Expected: FAIL on the first 4 (old script writes 4 files, no sentinel); last test may still pass on the old script.

- [ ] **Step 3: Replace `config()` with new script**

In `src/ci/pipeline_builder/base/git_diff_step.rs`, replace the entire `impl StepDef for GitDiffStep { fn config(&self) -> StepConfig { ... } }` block for `config()` (approx lines 23-60) with the following:

```rust
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

echo "git-diff: $TOTAL unique file(s) changed on current branch"
echo "  unstaged: $U"
echo "  staged: $S"
echo "  untracked: $T"
if [ -n "$BASE" ]; then echo "  branch-ahead (vs $BASE_LABEL): $B"; else echo "  branch-ahead: n/a (no base ref configured)"; fi
exit 1"#;

        let script = body.replace("__BASE_PREFIX__", &base_prefix);

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
        // TEMPORARY — rewritten in Task 4; leave single legacy entry so the
        // compile succeeds and Task 3 tests can run.
        ExceptionMapping::new(CallbackCommand::GitDiffCommand).add(
            "git_diff_changes_found",
            ExceptionEntry {
                command: CallbackCommand::GitDiffCommand,
                max_retries: 0,
                context_paths: vec!["pipelight-misc/git-diff-report/diff.txt".into()],
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, _stderr: &str) -> Option<String> {
        if stdout.contains("change record(s) on current branch")
            || stdout.contains("unique file(s) changed on current branch")
        {
            Some("git_diff_changes_found".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        // TEMPORARY — Task 5 tightens up the match strings; leave flexible for now.
        let output = format!("{}{}", stdout, stderr);
        if output.contains("not a git repository") { return "git-diff: skipped (no git repo)".into(); }
        if output.contains("working tree clean") { return "git-diff: skipped (tree clean)".into(); }
        if let Some(line) = output.lines().find(|l|
            l.contains("change record(s) on current branch")
            || l.contains("unique file(s) changed on current branch")) {
            return line.trim().to_string();
        }
        if success { "git-diff: ok".into() } else { "git-diff: failed".into() }
    }
}
```

*(Note: the intermediate `exception_mapping`, `match_exception`, and `output_report_str` above are stepping stones; Task 4 + Task 5 replace them cleanly. This keeps every commit compiling.)*

- [ ] **Step 4: Run Task 3 tests**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_script_writes_single_diff_txt ci::pipeline_builder::base::git_diff_step::tests::test_new_variant_uses_upstream ci::pipeline_builder::base::git_diff_step::tests::test_literal_variant_uses_given_ref ci::pipeline_builder::base::git_diff_step::tests::test_script_sentinel_present ci::pipeline_builder::base::git_diff_step::tests::test_script_still_detects_untracked_files`
Expected: PASS (5 tests).

- [ ] **Step 5: Remove the obsolete `test_context_paths_include_untracked` test**

In the test module, find and delete this block entirely:

```rust
#[test]
fn test_context_paths_include_untracked() {
    let step = GitDiffStep::new();
    let of = step.exception_mapping().to_on_failure();
    assert!(
        of.context_paths.iter().any(|p| p.contains("untracked.txt")),
        "context_paths should include untracked.txt"
    );
}
```

- [ ] **Step 6: Fix `test_exception_mapping_default_is_git_diff_command`**

Find the existing test:
```rust
#[test]
fn test_exception_mapping_default_is_git_diff_command() {
    let step = GitDiffStep::new();
    let of = step.exception_mapping().to_on_failure();
    assert_eq!(of.callback_command, CallbackCommand::GitDiffCommand);
    assert_eq!(of.max_retries, 0);
    assert_eq!(of.context_paths.len(), 4);
}
```

Change `context_paths.len()` from `4` to `1`.

- [ ] **Step 7: Fix `test_report_has_changes`**

Find:
```rust
let stdout = "git-diff: 6 change record(s) on current branch\n  unstaged: 2 file(s)\n  staged: 1 file(s)\n  untracked: 1 file(s)\n  unpushed (ahead of origin/main): 2 file(s)\n";
let r = step.output_report_str(false, stdout, "");
assert_eq!(r, "git-diff: 6 change record(s) on current branch");
```

Replace with:
```rust
let stdout = "git-diff: 6 unique file(s) changed on current branch\n  unstaged: 2\n  staged: 1\n  untracked: 1\n  branch-ahead (vs origin/main): 2\n";
let r = step.output_report_str(false, stdout, "");
assert_eq!(r, "git-diff: 6 unique file(s) changed on current branch");
```

- [ ] **Step 8: Fix `test_report_clean`**

Find:
```rust
let r = step.output_report_str(
    true,
    "git-diff: working tree clean and no unpushed commits — skipping\n",
    "",
);
```

Replace the input string with:
```rust
let r = step.output_report_str(
    true,
    "git-diff: working tree clean and no branch-ahead commits — skipping\n",
    "",
);
```

- [ ] **Step 9: Fix `test_exception_mapping_changes_found_key`**

Find the existing test and update the stdout simulation string from `"git-diff: 3 change record(s) on current branch\n"` to `"git-diff: 3 unique file(s) changed on current branch\n"` (the test asserts on `exception_key == "git_diff_changes_found"`, so both formats should still match after Task 3's lenient `match_exception`).

- [ ] **Step 10: Run full git_diff_step test suite**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step`
Expected: all tests pass (including the 5 new + existing updated).

- [ ] **Step 11: Run full build + clippy to catch type churn**

Run: `cargo build -p pipelight && cargo clippy -p pipelight -- -D warnings`
Expected: clean build, no clippy warnings.

- [ ] **Step 12: Commit**

```bash
git add src/ci/pipeline_builder/base/git_diff_step.rs
git commit -m "refactor(git-diff): unify output to single diff.txt + support base_ref variant"
```

---

## Task 4: Add `git_diff_base_not_found` exception + `RuntimeError` mapping

**Files:**
- Modify: `src/ci/pipeline_builder/base/git_diff_step.rs` (replace temporary `exception_mapping` / `match_exception` with final versions)

- [ ] **Step 1: Write failing tests**

Append to the test module:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_exception_mapping_base_not_found_entry_exists ci::pipeline_builder::base::git_diff_step::tests::test_match_exception_base_not_found_priority`
Expected: FAIL — `git_diff_base_not_found` key missing from mapping; `match_exception` returns `git_diff_changes_found` instead of `git_diff_base_not_found` for exit=2.

- [ ] **Step 3: Replace `exception_mapping` with final version**

In `src/ci/pipeline_builder/base/git_diff_step.rs`, replace the temporary `exception_mapping` introduced in Task 3 with:

```rust
fn exception_mapping(&self) -> ExceptionMapping {
    ExceptionMapping::new(CallbackCommand::GitDiffCommand)
        .add(
            "git_diff_changes_found",
            ExceptionEntry {
                command: CallbackCommand::GitDiffCommand,
                max_retries: 0,
                context_paths: vec!["pipelight-misc/git-diff-report/diff.txt".into()],
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
```

- [ ] **Step 4: Replace `match_exception` with priority-ordered version**

```rust
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
```

- [ ] **Step 5: Run the four new tests**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_exception_mapping_base_not_found_entry_exists ci::pipeline_builder::base::git_diff_step::tests::test_match_exception_base_not_found_priority ci::pipeline_builder::base::git_diff_step::tests::test_match_exception_changes_found_on_exit_1 ci::pipeline_builder::base::git_diff_step::tests::test_match_exception_no_match_on_clean`
Expected: PASS (4 tests).

- [ ] **Step 6: Verify `RuntimeError` registry action wiring**

Append one more test to the module:

```rust
#[test]
fn test_registry_action_for_base_not_found_is_runtime_error() {
    let registry = CallbackCommandRegistry::new();
    assert_eq!(
        registry.action_for(&CallbackCommand::RuntimeError),
        CallbackCommandAction::RuntimeError
    );
}
```

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_registry_action_for_base_not_found_is_runtime_error`
Expected: PASS.

- [ ] **Step 7: Run full git_diff_step suite**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add src/ci/pipeline_builder/base/git_diff_step.rs
git commit -m "feat(git-diff): add git_diff_base_not_found exception mapping to RuntimeError"
```

---

## Task 5: Tighten `output_report_str` + remove transitional leniency

**Files:**
- Modify: `src/ci/pipeline_builder/base/git_diff_step.rs` — replace transitional `output_report_str` and `match_exception` with strict final versions.

- [ ] **Step 1: Write a failing test for the tightened match_exception**

Append to the test module:

```rust
#[test]
fn test_match_exception_no_longer_matches_legacy_string() {
    let step = GitDiffStep::new();
    // After Task 5 we drop the legacy "change record(s)" phrasing. No real
    // script produces it anymore; make sure matcher only keys off the new text.
    let out = "git-diff: 3 change record(s) on current branch\n";
    let key = step.match_exception(1, out, "");
    assert_eq!(key, None, "legacy marker should no longer match");
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_match_exception_no_longer_matches_legacy_string`
Expected: FAIL — Task 3's `match_exception` still accepts the legacy string.

- [ ] **Step 3: Tighten `match_exception`**

Remove the `change record(s) on current branch` OR-branch added in Task 3:

```rust
fn match_exception(&self, exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
    if exit_code == 2 && stderr.contains("base ref") && stderr.contains("not found") {
        return Some("git_diff_base_not_found".into());
    }
    if stdout.contains("unique file(s) changed on current branch") {
        return Some("git_diff_changes_found".into());
    }
    None
}
```

*(This already matches what Task 4 wrote. If Task 4's version was already strict, this step is a no-op — verify the OR-branch is gone.)*

- [ ] **Step 4: Tighten `output_report_str`**

Replace with:

```rust
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
```

- [ ] **Step 5: Add a test for the base-not-found report string**

```rust
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
```

- [ ] **Step 6: Run the two updated/new tests**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_match_exception_no_longer_matches_legacy_string ci::pipeline_builder::base::git_diff_step::tests::test_output_report_str_base_not_found`
Expected: PASS (2 tests).

- [ ] **Step 7: Run full git_diff_step suite**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add src/ci/pipeline_builder/base/git_diff_step.rs
git commit -m "refactor(git-diff): tighten output_report_str + drop legacy marker"
```

---

## Task 6: Simplify `git_changed_files_snippet` to read single `diff.txt`

**Files:**
- Modify: `src/ci/pipeline_builder/mod.rs:375-402`

- [ ] **Step 1: Write failing tests**

Find the `#[cfg(test)] mod tests { ... }` block in `src/ci/pipeline_builder/mod.rs` (search for `mod tests`) and append:

```rust
#[test]
fn test_snippet_reads_single_diff_txt() {
    let snippet = git_changed_files_snippet(&["*.java"], None);
    assert!(
        snippet.contains("/workspace/pipelight-misc/git-diff-report/diff.txt"),
        "snippet must cat the unified diff.txt; got:\n{snippet}"
    );
    assert!(
        !snippet.contains("unstaged.txt")
            && !snippet.contains("staged.txt")
            && !snippet.contains("untracked.txt")
            && !snippet.contains("unpushed.txt"),
        "snippet must not reference legacy per-category files; got:\n{snippet}"
    );
}

#[test]
fn test_snippet_drops_redundant_sort_u() {
    let snippet = git_changed_files_snippet(&["*.java"], None);
    // diff.txt is already sort -u; downstream snippet shouldn't re-sort.
    assert!(
        !snippet.contains("| sort -u"),
        "snippet should drop its own sort -u since diff.txt is pre-sorted; got:\n{snippet}"
    );
}

#[test]
fn test_snippet_preserves_subdir_strip() {
    let snippet = git_changed_files_snippet(&["*.java"], Some("backend"));
    assert!(
        snippet.contains("sed 's|^backend/||'"),
        "snippet must still strip subdir prefix; got:\n{snippet}"
    );
}

#[test]
fn test_snippet_preserves_multi_ext_filter() {
    let snippet = git_changed_files_snippet(&["*.java", "*.kt"], None);
    assert!(
        snippet.contains("grep -E '\\.(java|kt)$'"),
        "snippet must preserve multi-ext filter; got:\n{snippet}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight ci::pipeline_builder::tests::test_snippet_reads_single_diff_txt ci::pipeline_builder::tests::test_snippet_drops_redundant_sort_u`
Expected: FAIL (snippet still cats 4 files and has `sort -u`).

- [ ] **Step 3: Rewrite `git_changed_files_snippet`**

In `src/ci/pipeline_builder/mod.rs` lines 375-402, replace the function body with:

```rust
pub fn git_changed_files_snippet(globs: &[&str], subdir: Option<&str>) -> String {
    // Convert glob patterns like "*.java", "*.kt" into grep -E regex: "\.(java|kt)$"
    let extensions: Vec<&str> = globs.iter().filter_map(|g| g.strip_prefix("*.")).collect();
    let grep_filter = if extensions.is_empty() {
        String::new()
    } else if extensions.len() == 1 {
        format!(" | grep -E '\\.{}$'", extensions[0])
    } else {
        format!(" | grep -E '\\.({})$'", extensions.join("|"))
    };

    let sed_strip = match subdir {
        Some(sd) => format!(" | sed 's|^{}/||'", sd),
        None => String::new(),
    };

    let report_file = "/workspace/pipelight-misc/git-diff-report/diff.txt";
    format!(
        "CHANGED_FILES=$( \
           cat {file} 2>/dev/null\
           {sed}{grep} \
           | while read f; do [ -f \"$f\" ] && echo \"$f\"; done \
         )",
        file = report_file,
        sed = sed_strip,
        grep = grep_filter,
    )
}
```

- [ ] **Step 4: Run the 4 new tests**

Run: `cargo test -p pipelight ci::pipeline_builder::tests::test_snippet_reads_single_diff_txt ci::pipeline_builder::tests::test_snippet_drops_redundant_sort_u ci::pipeline_builder::tests::test_snippet_preserves_subdir_strip ci::pipeline_builder::tests::test_snippet_preserves_multi_ext_filter`
Expected: PASS (4 tests).

- [ ] **Step 5: Run full pipeline_builder suite + downstream steps**

Run: `cargo test -p pipelight ci::pipeline_builder`
Expected: all tests pass. If any PMD/SpotBugs/JaCoCo step test asserts on the exact text of the shell snippet, update that test to match the new single-file form (e.g. search for tests that grep for `unstaged.txt` in step scripts).

- [ ] **Step 6: Commit**

```bash
git add src/ci/pipeline_builder/mod.rs
# Include any downstream step test updates if needed:
# git add src/ci/pipeline_builder/{maven,gradle}/*_step.rs
git commit -m "refactor(pipeline_builder): read single diff.txt in git_changed_files_snippet"
```

---

## Task 7: Add `--git-diff-from-remote-branch` flag to `Run` subcommand

**Files:**
- Modify: `src/cli/mod.rs:30-70` (flag declaration on `Command::Run`)
- Modify: `src/cli/mod.rs:157-202` (dispatch arm)

- [ ] **Step 1: Add the flag to `Command::Run`**

In `src/cli/mod.rs`, inside the `Command::Run { ... }` enum variant (around line 33-70), add the following field right after `full_report_only`:

```rust
/// Use the given remote ref (e.g. `origin/main`) as the branch-ahead base
/// for the git-diff step, replacing `@{upstream}`. Lets incremental
/// code-quality scans cover ALL files changed since the branch was cut
/// from a mainline branch. If the ref is not present locally, the pipeline
/// exits with a RuntimeError asking you to `git fetch` first.
#[arg(long = "git-diff-from-remote-branch", value_name = "REMOTE_BRANCH")]
git_diff_from_remote_branch: Option<String>,
```

- [ ] **Step 2: Add a sanity build**

Run: `cargo build -p pipelight`
Expected: COMPILE ERROR — the `Command::Run` destructure pattern in `dispatch` is missing the new field.

- [ ] **Step 3: Thread the field through `dispatch`**

In `src/cli/mod.rs` around line 158-181, update the `Command::Run` destructure and the `cmd_run` call to include the new field:

```rust
Command::Run {
    file,
    step,
    skip,
    dry_run,
    output,
    run_id,
    verbose,
    ping_pong,
    full_report_only,
    git_diff_from_remote_branch,
} => {
    cmd_run(
        file,
        step,
        skip,
        dry_run,
        output,
        run_id,
        verbose,
        ping_pong,
        full_report_only,
        git_diff_from_remote_branch,
    )
    .await
}
```

- [ ] **Step 4: Extend `cmd_run` signature**

In `src/cli/mod.rs` around line 204-215, extend `cmd_run`:

```rust
#[allow(clippy::too_many_arguments)]
async fn cmd_run(
    file: PathBuf,
    step_filter: Option<String>,
    skip_steps: Vec<String>,
    dry_run: bool,
    output: Option<String>,
    run_id: Option<String>,
    verbose: bool,
    ping_pong: bool,
    full_report_only: bool,
    git_diff_from_remote_branch: Option<String>,
) -> Result<i32> {
```

- [ ] **Step 5: Rewrite the git-diff step `commands[0]` when the flag is set**

Inside `cmd_run`, *right after* the `full_report_only` tag-activation block (around line 236) and *before* `let project_dir = ...` (around line 239), insert:

```rust
    // If --git-diff-from-remote-branch is set, overwrite the git-diff step's
    // script with the literal-base-ref variant. Falls through silently if the
    // pipeline has no "git-diff" step (e.g. init templates without it).
    if let Some(ref base) = git_diff_from_remote_branch {
        if let Some(step) = pipeline.steps.iter_mut().find(|s| s.name == "git-diff") {
            let new_cfg = crate::ci::pipeline_builder::base::GitDiffStep::with_base_ref(
                Some(base.clone()),
            )
            .config();
            step.commands = new_cfg.commands;
        }
    }
```

- [ ] **Step 6: Persist the flag into `RunState`**

Where `cmd_run` currently sets `state.full_report_only = full_report_only;` (around line 261), add the line directly after:

```rust
    state.full_report_only = full_report_only;
    state.git_diff_base = git_diff_from_remote_branch.clone();
```

- [ ] **Step 7: Import path check**

The line in Step 5 uses `crate::ci::pipeline_builder::base::GitDiffStep`. Verify this path resolves: scan the top of `src/cli/mod.rs` for existing imports. If `crate::ci::pipeline_builder::base` is not already in scope, the fully-qualified path works as written (no `use` change required).

- [ ] **Step 8: Build + run full test suite**

Run: `cargo build -p pipelight`
Expected: clean build.

Run: `cargo test -p pipelight`
Expected: all pass.

- [ ] **Step 9: Add a focused unit test in `cli/mod.rs`**

Find the test module inside `src/cli/mod.rs` and append:

```rust
#[test]
fn test_cli_parses_git_diff_from_remote_branch_flag() {
    use clap::Parser;
    let cli = Cli::try_parse_from([
        "pipelight",
        "run",
        "--git-diff-from-remote-branch=origin/main",
    ])
    .expect("should parse");
    match cli.command {
        Some(Command::Run {
            git_diff_from_remote_branch,
            ..
        }) => assert_eq!(git_diff_from_remote_branch.as_deref(), Some("origin/main")),
        _ => panic!("expected Run subcommand"),
    }
}

#[test]
fn test_cli_git_diff_flag_defaults_to_none() {
    use clap::Parser;
    let cli = Cli::try_parse_from(["pipelight", "run"]).expect("should parse");
    match cli.command {
        Some(Command::Run {
            git_diff_from_remote_branch,
            ..
        }) => assert_eq!(git_diff_from_remote_branch, None),
        _ => panic!("expected Run subcommand"),
    }
}
```

Run: `cargo test -p pipelight cli::tests::test_cli_parses_git_diff_from_remote_branch_flag cli::tests::test_cli_git_diff_flag_defaults_to_none`

*(If the test module is not named `tests` in `cli/mod.rs`, adjust the test path. Search for `#[cfg(test)]` inside that file to find the correct module name.)*

Expected: PASS (2 tests).

- [ ] **Step 10: Commit**

```bash
git add src/cli/mod.rs
git commit -m "feat(cli): add --git-diff-from-remote-branch flag to run subcommand"
```

---

## Task 8: Add `--git-diff-from-remote-branch` flag to `Retry` subcommand

**Files:**
- Modify: `src/cli/mod.rs` (`Command::Retry { ... }` variant around line 86-107, dispatch arm around line 185-194, `cmd_retry` signature around line 840)

- [ ] **Step 1: Add the flag to `Command::Retry`**

In the `Command::Retry { ... }` variant (around line 86-107), add after `verbose`:

```rust
/// Override the branch-ahead base ref used by the git-diff step. If omitted
/// on retry, the base ref persisted in the run state from the original run
/// is reused.
#[arg(long = "git-diff-from-remote-branch", value_name = "REMOTE_BRANCH")]
git_diff_from_remote_branch: Option<String>,
```

- [ ] **Step 2: Update dispatch arm**

In `dispatch` (around line 185-194):

```rust
Command::Retry {
    run_id,
    step,
    output,
    file,
    verbose,
    git_diff_from_remote_branch,
} => {
    let mode = resolve_output_mode(output);
    cmd_retry(run_id, step, mode, file, verbose, git_diff_from_remote_branch).await
}
```

- [ ] **Step 3: Extend `cmd_retry` signature**

```rust
async fn cmd_retry(
    run_id: String,
    step: Option<String>,
    mode: OutputMode,
    file: PathBuf,
    _verbose: bool,
    git_diff_from_remote_branch_override: Option<String>,
) -> Result<i32> {
```

- [ ] **Step 4: Reconcile override vs. persisted state**

Near the top of `cmd_retry`, right after `let mut state = RunState::load(&base, &run_id)?;` (around line 850), add:

```rust
    // Resolve effective base ref: explicit retry flag > persisted state value.
    if git_diff_from_remote_branch_override.is_some() {
        state.git_diff_base = git_diff_from_remote_branch_override.clone();
    }
    let effective_git_diff_base = state.git_diff_base.clone();
```

- [ ] **Step 5: Apply the base ref to the retry step if it's `git-diff`**

`cmd_retry` clones the single step to be retried (around line 895). If that step is `git-diff` and `effective_git_diff_base` is `Some`, overwrite its `commands`. After the block that sets `retry_step = pipeline.get_step(...).clone()` (around line 898), add:

```rust
    if retry_step.name == "git-diff" {
        if let Some(ref base) = effective_git_diff_base {
            let new_cfg = crate::ci::pipeline_builder::base::GitDiffStep::with_base_ref(
                Some(base.clone()),
            )
            .config();
            retry_step.commands = new_cfg.commands;
        }
    }
```

*(If retry targets a downstream quality step, no script rewrite is needed — those steps read `diff.txt` regardless of how it was produced.)*

- [ ] **Step 6: Ensure the updated `state.git_diff_base` is persisted**

Grep in `cmd_retry` for existing `state.save(` calls:

```bash
grep -n "state.save" src/cli/mod.rs
```

Verify at least one `state.save(...)` call runs after the mutation in Step 4. If so, no new save needed. If the only save happens *before* our mutation, insert `state.save(&base, &run_id)?;` immediately after the assignment in Step 4 so the new base ref is durable even if the retry errors out later.

- [ ] **Step 7: Write retry tests**

Append to the same test module as Task 7:

```rust
#[test]
fn test_cli_retry_parses_git_diff_flag() {
    use clap::Parser;
    let cli = Cli::try_parse_from([
        "pipelight",
        "retry",
        "--run-id=abc",
        "--step=git-diff",
        "--git-diff-from-remote-branch=origin/develop",
    ])
    .expect("should parse");
    match cli.command {
        Some(Command::Retry {
            git_diff_from_remote_branch,
            ..
        }) => assert_eq!(git_diff_from_remote_branch.as_deref(), Some("origin/develop")),
        _ => panic!("expected Retry subcommand"),
    }
}

#[test]
fn test_cli_retry_without_git_diff_flag() {
    use clap::Parser;
    let cli = Cli::try_parse_from([
        "pipelight",
        "retry",
        "--run-id=abc",
        "--step=pmd",
    ])
    .expect("should parse");
    match cli.command {
        Some(Command::Retry {
            git_diff_from_remote_branch,
            ..
        }) => assert_eq!(git_diff_from_remote_branch, None),
        _ => panic!("expected Retry subcommand"),
    }
}
```

- [ ] **Step 8: Build + run tests**

Run: `cargo build -p pipelight && cargo test -p pipelight cli::tests::test_cli_retry_parses_git_diff_flag cli::tests::test_cli_retry_without_git_diff_flag`
Expected: PASS (2 tests).

- [ ] **Step 9: Run full suite**

Run: `cargo test -p pipelight`
Expected: all pass.

- [ ] **Step 10: Commit**

```bash
git add src/cli/mod.rs
git commit -m "feat(cli): accept --git-diff-from-remote-branch on retry subcommand"
```

---

## Task 9: Final clippy + fmt sweep

**Files:** all touched in Tasks 1-8.

- [ ] **Step 1: Run fmt**

Run: `cargo fmt -p pipelight`
Expected: no output (or only re-formats consistent with repo style).

- [ ] **Step 2: Run clippy with warnings-as-errors**

Run: `cargo clippy -p pipelight --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: If anything was reformatted, commit it**

```bash
git status --short
# If modified files exist:
git add -u
git commit -m "chore: cargo fmt + clippy cleanup after git-diff-from-remote-branch integration"
```

---

## Task 10: Update `pipelight-run` skill docs

**Files:**
- Modify: `global-skills/pipelight-run/SKILL.md`

- [ ] **Step 1: Locate the flags section**

In `global-skills/pipelight-run/SKILL.md`, find the existing list of `pipelight run` flags (it should contain `--full-report-only` near the bottom). Grep marker: search for `--full-report-only`.

- [ ] **Step 2: Insert the new flag entry**

Right after the `--full-report-only` entry, add:

```markdown
- `--git-diff-from-remote-branch=<remote-branch>`: 指定远程分支作为 branch-ahead
  对比基准（如 `origin/main`），替代默认的 `@{upstream}`。用于在"从主分支切出的
  feature 分支"上只对自迁出以来改动过的文件运行代码质量扫描（PMD / SpotBugs /
  JaCoCo）。不传此 flag 时 pipelight 使用 `@{upstream}`，与原行为一致。
  - 典型用法：`pipelight run --git-diff-from-remote-branch=origin/main`
  - 要求 ref 本地已 fetch；不存在时 pipeline 终止并触发 `runtime_error` 回调，
    提示用户运行 `git fetch` 后重试
  - 值必须是完整的 remote ref（`origin/<branch>`），不要裸写分支名
```

- [ ] **Step 3: Update the "产物目录" / "pipelight-misc/" section**

Grep for `unstaged.txt` or `git-diff-report` in the file. If the skill lists the 4 per-category files, replace the list with:

```markdown
- `pipelight-misc/git-diff-report/diff.txt`: 单一汇总文档，包含当前分支所有变更文件
  的去重路径列表（sort -u 后）。生成于 git-diff step 执行后；LLM 阅读该文件即可了解
  本次扫描目标集合。step stdout 仍会打印分类统计（unstaged / staged / untracked /
  branch-ahead）供人类阅读。
```

- [ ] **Step 4: Verify no stale references to the 4 txt files remain**

Grep: `grep -nE 'unstaged\.txt|staged\.txt|untracked\.txt|unpushed\.txt' global-skills/pipelight-run/SKILL.md`
Expected: no matches.

- [ ] **Step 5: Sync to local skills directory (required by CLAUDE.md)**

Run: `cp -r global-skills/pipelight-run/ ~/.claude/skills/pipelight-run/`
Expected: files copied without errors.

- [ ] **Step 6: Commit**

```bash
git add global-skills/pipelight-run/SKILL.md
git commit -m "docs(skill): document --git-diff-from-remote-branch flag + unified diff.txt"
```

---

## Task 11: End-to-end manual verification

This task is NOT automated. Do it on a real Java/Rust project with a populated git history.

- [ ] **Step 1: Install current pipelight from the branch**

Run:
```bash
cargo install --path . --force
```
Expected: `pipelight` binary built and installed.

- [ ] **Step 2: In a repo with quality steps configured, initialize or refresh pipeline.yml**

```bash
cd /path/to/target/repo
pipelight clean
pipelight init
```

- [ ] **Step 3: Run with the new flag**

```bash
pipelight run --git-diff-from-remote-branch=origin/main
```

Expected observations:
- `pipelight-misc/git-diff-report/diff.txt` exists and contains deduplicated file paths covering everything changed since branching from `origin/main`.
- `pipelight-misc/git-diff-report/` does NOT contain `unstaged.txt`, `staged.txt`, `untracked.txt`, or `unpushed.txt` anymore.
- git-diff step stdout shows: `branch-ahead (vs origin/main): N`.
- PMD / SpotBugs / JaCoCo scan counts reflect the expanded file set (more than just unpushed commits).

- [ ] **Step 4: Verify error path**

```bash
pipelight run --git-diff-from-remote-branch=origin/nonexistent-branch
```

Expected:
- stderr contains `git-diff: base ref 'origin/nonexistent-branch' not found — run 'git fetch' first`.
- Pipeline terminates; `on_failure.action` is `"runtime_error"`.

- [ ] **Step 5: Verify default (no flag) still works**

```bash
pipelight run
```

Expected:
- Uses `@{upstream}` as before.
- git-diff step stdout shows: `branch-ahead (vs @{upstream}): N` (or `n/a` if no upstream).
- `diff.txt` still produced; content limited to uncommitted + unpushed.

- [ ] **Step 6: Verify retry inherits base**

```bash
RUN_ID=$(pipelight run --git-diff-from-remote-branch=origin/main --output=json | jq -r '.run_id')
pipelight retry --run-id=$RUN_ID --step=pmd
```

Expected:
- Retry re-uses `origin/main` as base (check by inspecting retry's logged git-diff output if step is rerun in the same pipeline, or by inspecting `state.git_diff_base` in `~/.pipelight/runs/<id>/state.json`).

- [ ] **Step 7: Record findings**

If anything diverges from expectations, file an issue comment or update the spec's "Open Questions" section before closing out the feature.

---

## Success Criteria

1. `cargo test -p pipelight` passes cleanly on all touched modules.
2. `cargo clippy -p pipelight --all-targets -- -D warnings` is clean.
3. On a real project with feature branch off `origin/main`, running `pipelight run --git-diff-from-remote-branch=origin/main` produces a `diff.txt` covering all files changed since the branch was cut, and PMD/SpotBugs/JaCoCo scan those files.
4. The default behavior (no flag) matches the pre-change behavior: `diff.txt` collects uncommitted + unpushed (vs `@{upstream}`) files only.
5. Specifying a non-existent remote ref fails fast with a clear `git fetch` hint and a `runtime_error` callback.
6. Retries inherit the original run's `git_diff_base` unless explicitly overridden on the retry command line.
7. The pipelight-run skill documents the new flag and the unified `diff.txt` output; the local `~/.claude/skills/pipelight-run/` copy is synced.
