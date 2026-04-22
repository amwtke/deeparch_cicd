# git-diff HTML Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enrich the existing `git_diff_report` callback action with an HTML rendering of per-file diffs, gated on `--git-diff-from-remote-branch`. Preserve all current markdown-terminal behavior when the flag is absent.

**Architecture:** The `git-diff` shell step writes a new `base-ref.txt` sidecar (alongside `diff.txt`) **only when `base_ref=Some`**; `exception_mapping()` conditionally includes the sidecar in `on_failure.context_paths`. A self-contained Python tool `gen_diff_html.py` — distributed with the `pipelight-run` skill — reads both files, shells out to `git diff <base> -- <file>` per path, renders a single inline-CSS `diff.html` with Pygments-highlighted `<details>` blocks, TOC, and file-type variants (tracked / untracked / binary / oversize). `/pipelight-sync` gains Python + Pygments detection with an auto-install ladder. SKILL.md gains an HTML branch in the `git_diff_report` flow.

**Tech Stack:** Rust (clap, serde), bash (shell step), Python 3.8+ with Pygments for the tool. Rust tests via `cargo test -p pipelight`. Python tests via `python3 -m unittest` (stdlib only, no pytest).

> **Note on `cargo test` invocations:**
> - `pipelight` is a binary-only crate. Do NOT pass `--lib` — it errors with "no library targets found". Use `cargo test -p pipelight <filter>` directly.
> - `cargo test` accepts only ONE positional filter. Where this plan lists multiple filters on a single line, collapse them to a shared parent-module prefix or invoke `cargo test` once per filter.

**Spec reference:** `docs/superpowers/specs/2026-04-22-git-diff-html-report-design.md`

---

## File Structure

**Created:**

- `global-skills/pipelight-run/tools/gen_diff_html.py` — the CLI rendering tool
- `global-skills/pipelight-run/tools/test_gen_diff_html.py` — unit tests (stdlib `unittest` + `unittest.mock`)

**Modified:**

- `src/ci/pipeline_builder/base/git_diff_step.rs` — shell emits `base-ref.txt`; `exception_mapping()` conditionally includes the sidecar
- `src/ci/callback/action.rs` — doc comment on `GitDiffReport` mentions the optional HTML path
- `.claude/skills/pipelight-sync/SKILL.md` — Step 2 gains `python3` + `pygments` detection with auto-install ladder
- `global-skills/pipelight-run/SKILL.md` — `git_diff_report` detailed flow gains the HTML branch

**Boundaries:**

- `GitDiffStep` owns only the shell text and exception mapping; it does NOT know about HTML or the py tool.
- `gen_diff_html.py` is self-contained: the only pipelight contract it reads is the two files (`diff.txt` + `base-ref.txt`).
- LLM dispatches via SKILL.md; pipelight Rust code is unaware of the HTML branch.
- `run_state.git_diff_base` already exists; no changes needed there.

---

## Task 1: Shell writes `base-ref.txt` when `base_ref=Some`

**Files:**
- Modify: `src/ci/pipeline_builder/base/git_diff_step.rs` — shell script body + tests

- [ ] **Step 1: Write the failing tests**

Append these tests to the existing `#[cfg(test)] mod tests` block in `src/ci/pipeline_builder/base/git_diff_step.rs`:

```rust
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
    // Ordering guarantee: the sidecar write must appear in the script text
    // AFTER the `exit 2` guard for BRANCH_AHEAD_ERR, so a broken ref never
    // leaves a stale base-ref.txt behind.
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_script_writes_base_ref_file_when_some`

Expected: FAIL — assertion fires because current script never writes `base-ref.txt`.

Also run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_base_ref_file_written_after_branch_ahead_err_guard`

Expected: FAIL — panic on `expect("sidecar write must be present for Some(ref)")`.

- [ ] **Step 3: Implement the sidecar write in the shell script**

Edit `src/ci/pipeline_builder/base/git_diff_step.rs`. The current shell `body` (starting around line 61) has this block near the bottom (exact current contents):

```
if [ "$BRANCH_AHEAD_ERR" = "1" ]; then exit 2; fi

if [ "$TOTAL" -eq 0 ]; then echo 'git-diff: working tree clean and no branch-ahead commits — skipping'; exit 0; fi

echo "git-diff: $TOTAL unique file(s) changed on current branch"
echo "  unstaged: $U"
echo "  staged: $S"
echo "  untracked: $T"
if [ -n "$BASE" ]; then echo "  branch-ahead (vs $BASE_LABEL): $B"; else echo "  branch-ahead: n/a (no base ref configured)"; fi
exit 1
```

Add a new conditional sidecar write **after** the `BRANCH_AHEAD_ERR` guard and **after** the clean-tree exit, so we only write the sidecar on the "changes found" success path. Replace the block above with:

```
if [ "$BRANCH_AHEAD_ERR" = "1" ]; then exit 2; fi

if [ "$TOTAL" -eq 0 ]; then echo 'git-diff: working tree clean and no branch-ahead commits — skipping'; exit 0; fi

__BASE_REF_SIDECAR__

echo "git-diff: $TOTAL unique file(s) changed on current branch"
echo "  unstaged: $U"
echo "  staged: $S"
echo "  untracked: $T"
if [ -n "$BASE" ]; then echo "  branch-ahead (vs $BASE_LABEL): $B"; else echo "  branch-ahead: n/a (no base ref configured)"; fi
exit 1
```

Then after the existing `script = body.replace("__BASE_PREFIX__", &base_prefix);` line, add:

```rust
let sidecar = match &self.base_ref {
    None => String::new(),
    Some(_) => r#"echo "$BASE" > "$REPORT_DIR/base-ref.txt""#.to_string(),
};
let script = script.replace("__BASE_REF_SIDECAR__", &sidecar);
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests`

Expected: all tests in the module pass (including the 3 new ones).

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/base/git_diff_step.rs
git commit -m "feat(git-diff): emit base-ref.txt sidecar when base_ref=Some

Shell step writes base-ref.txt alongside diff.txt only when a literal
base ref was configured (via --git-diff-from-remote-branch). Ordering
guarantees: sidecar write happens after BRANCH_AHEAD_ERR guard and
after the clean-tree early exit, so invalid-ref and no-changes paths
never leave a stale sidecar.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `exception_mapping` includes `base-ref.txt` when `base_ref=Some`

**Files:**
- Modify: `src/ci/pipeline_builder/base/git_diff_step.rs` — `exception_mapping()` method + tests

- [ ] **Step 1: Write the failing tests**

Append these to the same `#[cfg(test)] mod tests` block:

```rust
#[test]
fn test_context_paths_one_path_when_none() {
    let step = GitDiffStep::new();
    let of = step.exception_mapping().to_on_failure();
    assert_eq!(of.context_paths.len(), 1, "None variant must carry only diff.txt");
    assert_eq!(of.context_paths[0], "pipelight-misc/git-diff-report/diff.txt");
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests::test_context_paths_includes_base_ref_file_when_some`

Expected: FAIL — the current `exception_mapping()` always uses a single path.

- [ ] **Step 3: Update `exception_mapping()` to branch on `self.base_ref`**

Edit `src/ci/pipeline_builder/base/git_diff_step.rs`. Replace the existing `exception_mapping` implementation (currently at approximately lines 119-137) with:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight ci::pipeline_builder::base::git_diff_step::tests`

Expected: all module tests pass (including the 2 new ones + the existing `test_exception_mapping_default_is_git_diff_command` which asserts `len == 1` on the `new()` variant — verify it still passes).

- [ ] **Step 5: Update the `GitDiffReport` doc comment**

Edit `src/ci/callback/action.rs`. Replace the doc comment on the `GitDiffReport` variant (currently "LLM reads the three per-category file lists...") with:

```rust
/// LLM reads the deduplicated changed-files list (diff.txt) produced by
/// the git-diff step and prints a grouped summary to the terminal.
/// If the context also carries `base-ref.txt` (i.e. the user passed
/// `--git-diff-from-remote-branch=<ref>`), the LLM additionally runs the
/// bundled `gen_diff_html.py` tool to produce a self-contained
/// `diff.html` review artifact. Pipeline flow is unaffected in either case.
GitDiffReport,
```

- [ ] **Step 6: Run full test suite to catch regressions**

Run: `cargo test -p pipelight`

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/ci/pipeline_builder/base/git_diff_step.rs src/ci/callback/action.rs
git commit -m "feat(git-diff): include base-ref.txt in context_paths when set

exception_mapping() now pushes pipelight-misc/git-diff-report/base-ref.txt
as a second context path when base_ref=Some. The presence of this path
in on_failure.context_paths is the signal for the LLM to additionally
generate an HTML review artifact via the gen_diff_html.py tool.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Python tool skeleton — CLI, Pygments check, empty input

**Files:**
- Create: `global-skills/pipelight-run/tools/gen_diff_html.py`
- Create: `global-skills/pipelight-run/tools/test_gen_diff_html.py`

- [ ] **Step 1: Write the failing tests**

Create `global-skills/pipelight-run/tools/test_gen_diff_html.py`:

```python
"""Tests for gen_diff_html. Run with: python3 -m unittest test_gen_diff_html"""
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

HERE = Path(__file__).parent
TOOL = HERE / "gen_diff_html.py"


def run_tool(*args, cwd=None):
    """Invoke the tool as a subprocess; return CompletedProcess."""
    return subprocess.run(
        [sys.executable, str(TOOL), *args],
        capture_output=True,
        text=True,
        cwd=cwd,
    )


class TestSkeleton(unittest.TestCase):
    def test_missing_input_file_exits_1(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            base_ref = tmp / "base-ref.txt"
            base_ref.write_text("origin/main\n")
            result = run_tool(
                "--input", str(tmp / "nope.txt"),
                "--base-ref-file", str(base_ref),
                "--output", str(tmp / "out.html"),
                cwd=str(tmp),
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("diff.txt not found", result.stderr)

    def test_missing_base_ref_file_exits_1(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            diff = tmp / "diff.txt"
            diff.write_text("")
            result = run_tool(
                "--input", str(diff),
                "--base-ref-file", str(tmp / "nope.txt"),
                "--output", str(tmp / "out.html"),
                cwd=str(tmp),
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("base-ref.txt not found", result.stderr)

    def test_unsafe_base_ref_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "diff.txt").write_text("")
            (tmp / "base-ref.txt").write_text("; rm -rf /\n")
            result = run_tool(
                "--input", str(tmp / "diff.txt"),
                "--base-ref-file", str(tmp / "base-ref.txt"),
                "--output", str(tmp / "out.html"),
                cwd=str(tmp),
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("unsafe base ref", result.stderr)

    def test_empty_input_produces_empty_toc_html(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "diff.txt").write_text("")
            (tmp / "base-ref.txt").write_text("origin/main\n")
            out = tmp / "diff.html"
            result = run_tool(
                "--input", str(tmp / "diff.txt"),
                "--base-ref-file", str(tmp / "base-ref.txt"),
                "--output", str(out),
                cwd=str(tmp),
            )
            self.assertEqual(result.returncode, 0, msg=result.stderr)
            self.assertTrue(out.exists())
            html = out.read_text()
            self.assertIn("<!DOCTYPE html>", html)
            self.assertIn("origin/main", html)
            self.assertIn('class="toc"', html)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd global-skills/pipelight-run/tools
python3 -m unittest test_gen_diff_html -v
```

Expected: 4 errors (`gen_diff_html.py` does not yet exist — subprocess can't find it).

- [ ] **Step 3: Create the skeleton**

Create `global-skills/pipelight-run/tools/gen_diff_html.py`:

```python
#!/usr/bin/env python3
"""Generate a self-contained HTML diff report from pipelight's git-diff output.

Invoked by the LLM when processing a `git_diff_report` callback action whose
`on_failure.context_paths` includes `base-ref.txt` (i.e. the user ran
pipelight with --git-diff-from-remote-branch=<ref>).

Reads:
  --input          pipelight-misc/git-diff-report/diff.txt (paths, one per line)
  --base-ref-file  pipelight-misc/git-diff-report/base-ref.txt (single line)

Writes:
  --output         pipelight-misc/git-diff-report/diff.html (single file)
"""
import argparse
import html
import re
import subprocess
import sys
from datetime import datetime
from pathlib import Path

# ASCII whitelist mirroring the Rust side's debug_assert!.
SAFE_REF_RE = re.compile(r"^[A-Za-z0-9/_.-]+$")

# If `git diff` emits more than this many bytes for a single file, we
# replace the body with a "too large" placeholder.
MAX_DIFF_BYTES = 500 * 1024


def die(msg: str, code: int = 1) -> None:
    print(msg, file=sys.stderr)
    sys.exit(code)


def require_pygments():
    try:
        import pygments  # noqa: F401
    except ImportError:
        die(
            "Pygments not installed. "
            "Run: python3 -m pip install --user pygments",
            code=1,
        )


def parse_args(argv):
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--input", required=True, help="path to diff.txt")
    p.add_argument("--base-ref-file", required=True, help="path to base-ref.txt")
    p.add_argument("--output", required=True, help="path to write diff.html")
    p.add_argument("--cwd", default=".", help="repo root for git commands (default: CWD)")
    return p.parse_args(argv)


def read_base_ref(path: Path) -> str:
    if not path.exists():
        die(f"base-ref.txt not found at {path}")
    ref = path.read_text(encoding="utf-8").strip()
    if not SAFE_REF_RE.match(ref):
        die(f"unsafe base ref '{ref}' — may be tampered")
    return ref


def read_diff_paths(path: Path) -> list[str]:
    if not path.exists():
        die(f"diff.txt not found at {path}")
    return [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def render_html(base_ref: str, files: list[dict]) -> str:
    now = datetime.now().astimezone().strftime("%Y-%m-%d %H:%M:%S %Z")
    # Minimal skeleton for Task 3; later tasks fill in CSS, TOC, file blocks.
    parts = [
        "<!DOCTYPE html>",
        '<html lang="zh-CN"><head><meta charset="utf-8">',
        f"<title>git-diff report vs {html.escape(base_ref)}</title>",
        "<style>/* inline CSS added in Task 10 */</style>",
        "</head><body>",
        "<header>",
        "<h1>git-diff report</h1>",
        '<div class="meta">',
        f"<div>Base: <code>{html.escape(base_ref)}</code></div>",
        f"<div>Generated: {html.escape(now)}</div>",
        f"<div>Files: {len(files)}</div>",
        "</div>",
        "</header>",
        '<section class="toc"><h2>Files</h2><ul>',
    ]
    for f in files:
        parts.append(
            f'<li><a href="#{html.escape(f["anchor"])}">{html.escape(f["path"])}</a></li>'
        )
    parts += [
        "</ul></section>",
        '<section class="files"></section>',  # real blocks rendered from Task 4
        "</body></html>",
    ]
    return "\n".join(parts)


def main(argv=None) -> int:
    args = parse_args(argv)
    require_pygments()
    input_path = Path(args.input)
    base_ref_path = Path(args.base_ref_file)
    output_path = Path(args.output)
    cwd = Path(args.cwd)  # noqa: F841 — used by later tasks

    if not input_path.exists():
        die(f"diff.txt not found at {input_path}")
    base_ref = read_base_ref(base_ref_path)
    paths = read_diff_paths(input_path)

    # Task 3 scope: just produce a skeleton with TOC of paths, no file bodies.
    files = [{"path": p, "anchor": "f-" + re.sub(r"[^A-Za-z0-9]+", "-", p).strip("-").lower()} for p in paths]
    output_path.write_text(render_html(base_ref, files), encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

Note: the `require_pygments()` call happens early so Pygments-missing is the first detectable failure once inputs are validated.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd global-skills/pipelight-run/tools
python3 -m unittest test_gen_diff_html -v
```

Expected: 4 passes.

- [ ] **Step 5: Commit**

```bash
git add global-skills/pipelight-run/tools/gen_diff_html.py global-skills/pipelight-run/tools/test_gen_diff_html.py
git commit -m "feat(tools): gen_diff_html skeleton — CLI, pygments check, TOC

Minimal viable skeleton: parses --input/--base-ref-file/--output/--cwd,
validates both input files exist, rejects unsafe base ref chars, requires
Pygments, and writes an HTML file with header metadata and a TOC of paths
from diff.txt. File bodies (Task 4+) are empty for now.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Render tracked-file diff bodies

**Files:**
- Modify: `global-skills/pipelight-run/tools/gen_diff_html.py`
- Modify: `global-skills/pipelight-run/tools/test_gen_diff_html.py`

- [ ] **Step 1: Write the failing tests**

Append this class to `test_gen_diff_html.py`:

```python
class TestTrackedFileRender(unittest.TestCase):
    def _make_tmp(self, tmp: Path, diff_lines: list[str]):
        (tmp / "diff.txt").write_text("\n".join(diff_lines) + "\n")
        (tmp / "base-ref.txt").write_text("origin/main\n")

    def test_tracked_file_renders_details_and_hunk(self):
        fake_diff = (
            "diff --git a/src/foo.py b/src/foo.py\n"
            "index 1111111..2222222 100644\n"
            "--- a/src/foo.py\n"
            "+++ b/src/foo.py\n"
            "@@ -1,3 +1,4 @@\n"
            " context line\n"
            "-old line\n"
            "+new line\n"
            "+added line\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            self._make_tmp(tmp, ["src/foo.py"])
            # Patch subprocess.run to fake both `git ls-files` (tracked check)
            # and `git diff` (content). Use side_effect to branch per-call.
            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                if cmd[:2] == ["git", "ls-files"]:
                    # tracked file present
                    return SimpleNamespace(returncode=0, stdout="src/foo.py\n", stderr="")
                if cmd[:2] == ["git", "diff"]:
                    return SimpleNamespace(returncode=0, stdout=fake_diff, stderr="")
                return SimpleNamespace(returncode=0, stdout="", stderr="")

            with patch("subprocess.run", side_effect=fake_run):
                import importlib
                import gen_diff_html as mod
                importlib.reload(mod)
                rc = mod.main([
                    "--input", str(tmp / "diff.txt"),
                    "--base-ref-file", str(tmp / "base-ref.txt"),
                    "--output", str(tmp / "diff.html"),
                    "--cwd", str(tmp),
                ])
            self.assertEqual(rc, 0)
            html_out = (tmp / "diff.html").read_text()
            self.assertIn('<details id="f-src-foo-py"', html_out)
            self.assertIn("src/foo.py", html_out)
            # Hunk header preserved
            self.assertIn("@@ -1,3 +1,4 @@", html_out)
            # Line classes applied
            self.assertIn('class="line add"', html_out)
            self.assertIn('class="line del"', html_out)
            self.assertIn('class="line ctx"', html_out)
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd global-skills/pipelight-run/tools
python3 -m unittest test_gen_diff_html.TestTrackedFileRender -v
```

Expected: FAIL — current tool writes an empty `<section class="files"></section>`.

- [ ] **Step 3: Implement tracked-file rendering**

Edit `gen_diff_html.py`. Add these helper functions **above** `render_html`:

```python
def git_output(cmd: list[str], cwd: Path) -> tuple[int, str, str]:
    """Run a git command; return (returncode, stdout, stderr). Never raises."""
    r = subprocess.run(cmd, cwd=str(cwd), capture_output=True, text=True)
    return r.returncode, r.stdout, r.stderr


def is_tracked(path: str, base_ref: str, cwd: Path) -> bool:
    """True if git knows this path at HEAD or in the base ref."""
    rc, out, _ = git_output(["git", "ls-files", "--error-unmatch", "--", path], cwd)
    return rc == 0


def get_diff(path: str, base_ref: str, cwd: Path) -> tuple[str, str]:
    """Return (stdout, stderr) from `git diff <base> -- <path>`."""
    _rc, out, err = git_output(["git", "diff", base_ref, "--", path], cwd)
    return out, err


def parse_unified_diff(diff_text: str) -> list[dict]:
    """Parse a unified-diff text into a list of hunks.

    Each hunk: {"header": "@@ ...", "lines": [("add"|"del"|"ctx", "...content..."), ...]}
    """
    hunks = []
    current = None
    for line in diff_text.splitlines():
        if line.startswith("@@"):
            if current:
                hunks.append(current)
            current = {"header": line, "lines": []}
            continue
        if current is None:
            # Skip diff --git / index / --- / +++ headers
            continue
        if line.startswith("+"):
            current["lines"].append(("add", line[1:]))
        elif line.startswith("-"):
            current["lines"].append(("del", line[1:]))
        else:
            current["lines"].append(("ctx", line[1:] if line.startswith(" ") else line))
    if current:
        hunks.append(current)
    return hunks


def count_stats(hunks: list[dict]) -> tuple[int, int]:
    add = sum(1 for h in hunks for kind, _ in h["lines"] if kind == "add")
    dele = sum(1 for h in hunks for kind, _ in h["lines"] if kind == "del")
    return add, dele


def render_tracked_body(hunks: list[dict]) -> str:
    out = ['<div class="diff-body">']
    for h in hunks:
        out.append('<div class="hunk">')
        out.append(f'<div class="hunk-header">{html.escape(h["header"])}</div>')
        out.append('<pre class="code"><code>')
        for kind, content in h["lines"]:
            prefix = {"add": "+", "del": "-", "ctx": " "}[kind]
            out.append(
                f'<div class="line {kind}">'
                f'<span class="gutter">{prefix}</span>'
                f'<span class="content">{html.escape(content)}</span>'
                f'</div>'
            )
        out.append("</code></pre>")
        out.append("</div>")
    out.append("</div>")
    return "".join(out)


def render_file_block(path: str, anchor: str, base_ref: str, cwd: Path) -> tuple[str, str]:
    """Return (toc_li_html, details_block_html) for one file."""
    if is_tracked(path, base_ref, cwd):
        diff_text, _err = get_diff(path, base_ref, cwd)
        hunks = parse_unified_diff(diff_text)
        add, dele = count_stats(hunks)
        body = render_tracked_body(hunks)
        toc = (
            f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
            f'<span class="stat">+{add} &minus;{dele}</span></li>'
        )
        det = (
            f'<details id="{html.escape(anchor)}">'
            f'<summary><span class="path">{html.escape(path)}</span>'
            f' <span class="stat">+{add} &minus;{dele}</span></summary>'
            f'{body}'
            f'</details>'
        )
        return toc, det
    # Untracked, binary, oversize branches come in later tasks. For now,
    # fall through to an untracked-style placeholder so Task 4 is self-consistent.
    toc = f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a></li>'
    det = (
        f'<details id="{html.escape(anchor)}">'
        f'<summary><span class="path">{html.escape(path)}</span></summary>'
        f'</details>'
    )
    return toc, det
```

Now modify `render_html` to use `render_file_block`. Replace the body of `render_html` with:

```python
def render_html(base_ref: str, paths: list[str], cwd: Path) -> str:
    now = datetime.now().astimezone().strftime("%Y-%m-%d %H:%M:%S %Z")
    # Anchor collision handling will be added in Task 9; for now, slug-only.
    seen = {}
    files = []
    for p in paths:
        slug = "f-" + re.sub(r"[^A-Za-z0-9]+", "-", p).strip("-").lower()
        seen[slug] = seen.get(slug, 0) + 1
        if seen[slug] > 1:
            slug = f"{slug}-{seen[slug]}"
        files.append((p, slug))

    toc_items = []
    file_blocks = []
    for path, anchor in files:
        toc_li, det = render_file_block(path, anchor, base_ref, cwd)
        toc_items.append(toc_li)
        file_blocks.append(det)

    parts = [
        "<!DOCTYPE html>",
        '<html lang="zh-CN"><head><meta charset="utf-8">',
        f"<title>git-diff report vs {html.escape(base_ref)}</title>",
        "<style>/* inline CSS added in Task 10 */</style>",
        "</head><body>",
        "<header>",
        "<h1>git-diff report</h1>",
        '<div class="meta">',
        f"<div>Base: <code>{html.escape(base_ref)}</code></div>",
        f"<div>Generated: {html.escape(now)}</div>",
        f"<div>Files: {len(files)}</div>",
        "</div>",
        "</header>",
        '<section class="toc"><h2>Files</h2><ul>',
        *toc_items,
        "</ul></section>",
        '<section class="files">',
        *file_blocks,
        "</section>",
        "</body></html>",
    ]
    return "\n".join(parts)
```

Update `main` to pass `cwd`:

```python
def main(argv=None) -> int:
    args = parse_args(argv)
    require_pygments()
    input_path = Path(args.input)
    base_ref_path = Path(args.base_ref_file)
    output_path = Path(args.output)
    cwd = Path(args.cwd)

    base_ref = read_base_ref(base_ref_path)
    paths = read_diff_paths(input_path)

    output_path.write_text(render_html(base_ref, paths, cwd), encoding="utf-8")
    return 0
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd global-skills/pipelight-run/tools
python3 -m unittest test_gen_diff_html -v
```

Expected: all tests pass (existing 4 + 1 new = 5).

- [ ] **Step 5: Commit**

```bash
git add global-skills/pipelight-run/tools/gen_diff_html.py global-skills/pipelight-run/tools/test_gen_diff_html.py
git commit -m "feat(tools): render tracked-file diff bodies

Shells out to git diff <base> -- <file> per tracked path, parses unified
diff into hunks with add/del/ctx line classification, renders each file
as <details> with hunk-header + line-level span markup. Anchor slug is
path-normalized + de-duplicated within a single run.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Pygments syntax highlighting for tracked bodies

**Files:**
- Modify: `global-skills/pipelight-run/tools/gen_diff_html.py`
- Modify: `global-skills/pipelight-run/tools/test_gen_diff_html.py`

- [ ] **Step 1: Write the failing test**

Append to `test_gen_diff_html.py` inside `TestTrackedFileRender`:

```python
    def test_tracked_file_has_pygments_span(self):
        fake_diff = (
            "diff --git a/src/foo.py b/src/foo.py\n"
            "--- a/src/foo.py\n"
            "+++ b/src/foo.py\n"
            "@@ -1,1 +1,1 @@\n"
            "-def old(): pass\n"
            "+def new(): return 42\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            self._make_tmp(tmp, ["src/foo.py"])

            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                if cmd[:2] == ["git", "ls-files"]:
                    return SimpleNamespace(returncode=0, stdout="src/foo.py\n", stderr="")
                if cmd[:2] == ["git", "diff"]:
                    return SimpleNamespace(returncode=0, stdout=fake_diff, stderr="")
                return SimpleNamespace(returncode=0, stdout="", stderr="")

            with patch("subprocess.run", side_effect=fake_run):
                import importlib
                import gen_diff_html as mod
                importlib.reload(mod)
                rc = mod.main([
                    "--input", str(tmp / "diff.txt"),
                    "--base-ref-file", str(tmp / "base-ref.txt"),
                    "--output", str(tmp / "diff.html"),
                    "--cwd", str(tmp),
                ])
            self.assertEqual(rc, 0)
            html_out = (tmp / "diff.html").read_text()
            # Pygments emits <span class="..."> tokens for Python keywords
            # ("def" gets `k` (keyword) class by default).
            self.assertRegex(html_out, r'<span class="[^"]*\bk\b[^"]*">def</span>')

    def test_unknown_extension_falls_back_to_text_lexer(self):
        fake_diff = (
            "diff --git a/x.unknown b/x.unknown\n"
            "--- a/x.unknown\n"
            "+++ b/x.unknown\n"
            "@@ -1,1 +1,1 @@\n"
            "-aaa\n"
            "+bbb\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            self._make_tmp(tmp, ["x.unknown"])

            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                if cmd[:2] == ["git", "ls-files"]:
                    return SimpleNamespace(returncode=0, stdout="x.unknown\n", stderr="")
                if cmd[:2] == ["git", "diff"]:
                    return SimpleNamespace(returncode=0, stdout=fake_diff, stderr="")
                return SimpleNamespace(returncode=0, stdout="", stderr="")

            with patch("subprocess.run", side_effect=fake_run):
                import importlib
                import gen_diff_html as mod
                importlib.reload(mod)
                rc = mod.main([
                    "--input", str(tmp / "diff.txt"),
                    "--base-ref-file", str(tmp / "base-ref.txt"),
                    "--output", str(tmp / "diff.html"),
                    "--cwd", str(tmp),
                ])
            self.assertEqual(rc, 0)
            html_out = (tmp / "diff.html").read_text()
            # Fallback: TextLexer still emits <span> but no language-specific classes.
            # Simply assert that aaa and bbb show up (escaped) in the output.
            self.assertIn("aaa", html_out)
            self.assertIn("bbb", html_out)
```

- [ ] **Step 2: Run test to verify it fails**

```bash
python3 -m unittest test_gen_diff_html.TestTrackedFileRender.test_tracked_file_has_pygments_span -v
```

Expected: FAIL — current output has no Pygments `<span class="k">`.

- [ ] **Step 3: Implement Pygments highlighting**

Edit `gen_diff_html.py`. Add these imports near the top (after existing `from pathlib import Path`):

```python
from pygments import highlight
from pygments.formatters import HtmlFormatter
from pygments.lexers import get_lexer_by_name, get_lexer_for_filename, guess_lexer
from pygments.util import ClassNotFound
```

(Note: Pygments imports must come AFTER `require_pygments()` returns, so keep `require_pygments()` at the top of `main()`. But the module-level import is fine because `require_pygments` runs before anyone calls highlighting functions — and if pygments is missing, we die before touching them. To make this robust, wrap the top-level import in a try/except and re-import inside the highlight function… but that's over-engineering. The simpler contract: the tool REQUIRES pygments; importing at module load and getting `ImportError` there is just as valid as failing in `require_pygments()`. Remove `require_pygments()` entirely in favor of the top-level import raising a clear error.)

Replace `require_pygments()` body with:

```python
def require_pygments():
    """No-op: Pygments is imported at module top; a missing import raises ImportError
    with a clear hint handled in main()."""
```

And in `main()`, wrap the entry point:

```python
def main(argv=None) -> int:
    args = parse_args(argv)
    input_path = Path(args.input)
    base_ref_path = Path(args.base_ref_file)
    output_path = Path(args.output)
    cwd = Path(args.cwd)
    base_ref = read_base_ref(base_ref_path)
    paths = read_diff_paths(input_path)
    output_path.write_text(render_html(base_ref, paths, cwd), encoding="utf-8")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except ImportError as e:
        if "pygments" in str(e).lower():
            die("Pygments not installed. Run: python3 -m pip install --user pygments", code=1)
        raise
```

Also: keep the older `test_pygments_missing_exits_1`-style check working — the first test (`test_missing_input_file_exits_1`) does NOT depend on pygments because argparse runs first. But our new top-level `from pygments import ...` means even missing-input errors need pygments installed to run the tool. That's fine — the sidecar is only produced when the Pygments-available install ran via `/pipelight-sync`.

Now add a helper to pick a lexer + highlight:

```python
def pick_lexer(path: str):
    try:
        return get_lexer_for_filename(path, stripnl=False)
    except ClassNotFound:
        return get_lexer_by_name("text", stripnl=False)


def highlight_content(content: str, lexer) -> str:
    """Highlight a single logical line. Returns inline HTML without wrapping <pre>."""
    formatter = HtmlFormatter(nowrap=True)
    return highlight(content, lexer, formatter).rstrip("\n")
```

Modify `render_tracked_body` to take a `path` arg and use highlighting:

```python
def render_tracked_body(path: str, hunks: list[dict]) -> str:
    lexer = pick_lexer(path)
    out = ['<div class="diff-body">']
    for h in hunks:
        out.append('<div class="hunk">')
        out.append(f'<div class="hunk-header">{html.escape(h["header"])}</div>')
        out.append('<pre class="code"><code>')
        for kind, content in h["lines"]:
            prefix = {"add": "+", "del": "-", "ctx": " "}[kind]
            highlighted = highlight_content(content, lexer)
            out.append(
                f'<div class="line {kind}">'
                f'<span class="gutter">{prefix}</span>'
                f'<span class="content">{highlighted}</span>'
                f'</div>'
            )
        out.append("</code></pre>")
        out.append("</div>")
    out.append("</div>")
    return "".join(out)
```

And update `render_file_block`'s call:

```python
        body = render_tracked_body(path, hunks)
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
python3 -m unittest test_gen_diff_html -v
```

Expected: all 7 tests pass.

One caveat: the existing `test_tracked_file_renders_details_and_hunk` (from Task 4) asserts `'class="line add"'` shows up literally. With Pygments now wrapping content in additional spans, that class still exists on the outer `<div>`, so the test stays green. Verify the assertion text does not require an exact structure around the content.

- [ ] **Step 5: Commit**

```bash
git add global-skills/pipelight-run/tools/gen_diff_html.py global-skills/pipelight-run/tools/test_gen_diff_html.py
git commit -m "feat(tools): syntax highlighting via Pygments

Each diff line's content is highlighted with a filename-derived lexer;
unknown extensions fall back to TextLexer (no highlighting, just HTML
escape). ImportError on missing pygments is caught at the top-level
entry point with an actionable install hint.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Untracked file rendering (name + line count, no body)

**Files:**
- Modify: `global-skills/pipelight-run/tools/gen_diff_html.py`
- Modify: `global-skills/pipelight-run/tools/test_gen_diff_html.py`

- [ ] **Step 1: Write the failing test**

Append to `test_gen_diff_html.py`:

```python
class TestUntrackedFile(unittest.TestCase):
    def test_untracked_file_renders_summary_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "diff.txt").write_text("new_file.txt\n")
            (tmp / "base-ref.txt").write_text("origin/main\n")
            # Simulate a working-tree file so line-count works:
            (tmp / "new_file.txt").write_text("line1\nline2\nline3\n")

            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                if cmd[:2] == ["git", "ls-files"]:
                    # file is NOT tracked
                    return SimpleNamespace(returncode=1, stdout="", stderr="error: pathspec")
                return SimpleNamespace(returncode=0, stdout="", stderr="")

            with patch("subprocess.run", side_effect=fake_run):
                import importlib
                import gen_diff_html as mod
                importlib.reload(mod)
                rc = mod.main([
                    "--input", str(tmp / "diff.txt"),
                    "--base-ref-file", str(tmp / "base-ref.txt"),
                    "--output", str(tmp / "diff.html"),
                    "--cwd", str(tmp),
                ])
            self.assertEqual(rc, 0)
            html_out = (tmp / "diff.html").read_text()
            # TOC badge
            self.assertIn('class="stat badge-new"', html_out)
            # Summary text with line count
            self.assertIn("+3 lines (new file)", html_out)
            # No diff-body
            block_start = html_out.index('<details id="f-new-file-txt"')
            block_end = html_out.index("</details>", block_start)
            block = html_out[block_start:block_end]
            self.assertNotIn("diff-body", block, "untracked block must not have diff body")
```

- [ ] **Step 2: Run test to verify it fails**

```bash
python3 -m unittest test_gen_diff_html.TestUntrackedFile -v
```

Expected: FAIL — current fallthrough produces empty details without `badge-new` or `(new file)`.

- [ ] **Step 3: Implement untracked branch**

Edit `gen_diff_html.py`. Add a helper to count file lines:

```python
def count_file_lines(path: str, cwd: Path) -> int:
    fp = cwd / path
    if not fp.exists() or not fp.is_file():
        return 0
    try:
        with fp.open("r", encoding="utf-8", errors="replace") as f:
            return sum(1 for _ in f)
    except OSError:
        return 0
```

Modify `render_file_block` — replace the fallthrough branch (after `if is_tracked(...)`) with:

```python
    # Untracked: summary-only block with line count.
    line_count = count_file_lines(path, cwd)
    toc = (
        f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
        f'<span class="stat badge-new">new</span></li>'
    )
    det = (
        f'<details id="{html.escape(anchor)}">'
        f'<summary><span class="path">{html.escape(path)}</span>'
        f' <span class="stat badge-new">+{line_count} lines (new file)</span></summary>'
        f'</details>'
    )
    return toc, det
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
python3 -m unittest test_gen_diff_html -v
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add global-skills/pipelight-run/tools/gen_diff_html.py global-skills/pipelight-run/tools/test_gen_diff_html.py
git commit -m "feat(tools): render untracked files as summary-only blocks

Untracked (git ls-files returns non-zero) files show up in the TOC with
a 'new' badge, and their <details> block has a summary with line count
but no body. Per spec — untracked files are typically new source; a
reviewer wants to know they exist, not read the whole file in a diff view.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Binary file detection

**Files:**
- Modify: `global-skills/pipelight-run/tools/gen_diff_html.py`
- Modify: `global-skills/pipelight-run/tools/test_gen_diff_html.py`

- [ ] **Step 1: Write the failing test**

Append to `test_gen_diff_html.py`:

```python
class TestBinaryFile(unittest.TestCase):
    def test_binary_file_detected_and_omitted(self):
        fake_diff = (
            "diff --git a/logo.png b/logo.png\n"
            "index abc..def 100644\n"
            "Binary files a/logo.png and b/logo.png differ\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "diff.txt").write_text("logo.png\n")
            (tmp / "base-ref.txt").write_text("origin/main\n")

            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                if cmd[:2] == ["git", "ls-files"]:
                    return SimpleNamespace(returncode=0, stdout="logo.png\n", stderr="")
                if cmd[:2] == ["git", "diff"]:
                    return SimpleNamespace(returncode=0, stdout=fake_diff, stderr="")
                return SimpleNamespace(returncode=0, stdout="", stderr="")

            with patch("subprocess.run", side_effect=fake_run):
                import importlib
                import gen_diff_html as mod
                importlib.reload(mod)
                rc = mod.main([
                    "--input", str(tmp / "diff.txt"),
                    "--base-ref-file", str(tmp / "base-ref.txt"),
                    "--output", str(tmp / "diff.html"),
                    "--cwd", str(tmp),
                ])
            self.assertEqual(rc, 0)
            html_out = (tmp / "diff.html").read_text()
            self.assertIn("binary file, diff omitted", html_out)
            # No hunk header should appear
            self.assertNotIn("@@", html_out.split('<details id="f-logo-png"')[1].split("</details>")[0])
```

- [ ] **Step 2: Run test to verify it fails**

```bash
python3 -m unittest test_gen_diff_html.TestBinaryFile -v
```

Expected: FAIL — current tracked branch would try to parse hunks (which there are none) and render an empty body.

- [ ] **Step 3: Detect and branch for binary files**

Edit `gen_diff_html.py`. Add a detector:

```python
BINARY_MARKER = "Binary files "


def is_binary_diff(diff_text: str) -> bool:
    return BINARY_MARKER in diff_text and "@@" not in diff_text
```

Modify `render_file_block`. Replace the tracked branch (the entire `if is_tracked(path, base_ref, cwd):` block up to the `return toc, det` for that branch) with the version below that checks for binary BEFORE parsing hunks. The untracked fallthrough branch added in Task 6 is unchanged.

```python
def render_file_block(path: str, anchor: str, base_ref: str, cwd: Path) -> tuple[str, str]:
    if is_tracked(path, base_ref, cwd):
        diff_text, _err = get_diff(path, base_ref, cwd)
        if is_binary_diff(diff_text):
            toc = (
                f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
                f'<span class="stat">binary</span></li>'
            )
            det = (
                f'<details id="{html.escape(anchor)}">'
                f'<summary><span class="path">{html.escape(path)}</span>'
                f' <span class="stat">binary</span></summary>'
                f'<div class="diff-body"><p>binary file, diff omitted</p></div>'
                f'</details>'
            )
            return toc, det
        hunks = parse_unified_diff(diff_text)
        add, dele = count_stats(hunks)
        body = render_tracked_body(path, hunks)
        toc = (
            f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
            f'<span class="stat">+{add} &minus;{dele}</span></li>'
        )
        det = (
            f'<details id="{html.escape(anchor)}">'
            f'<summary><span class="path">{html.escape(path)}</span>'
            f' <span class="stat">+{add} &minus;{dele}</span></summary>'
            f'{body}'
            f'</details>'
        )
        return toc, det
    # Untracked branch (from Task 6): name + line count only, no body.
    line_count = count_file_lines(path, cwd)
    toc = (
        f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
        f'<span class="stat badge-new">new</span></li>'
    )
    det = (
        f'<details id="{html.escape(anchor)}">'
        f'<summary><span class="path">{html.escape(path)}</span>'
        f' <span class="stat badge-new">+{line_count} lines (new file)</span></summary>'
        f'</details>'
    )
    return toc, det
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
python3 -m unittest test_gen_diff_html -v
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add global-skills/pipelight-run/tools/gen_diff_html.py global-skills/pipelight-run/tools/test_gen_diff_html.py
git commit -m "feat(tools): detect and omit binary file diffs

If git's unified-diff output contains 'Binary files ' and no hunk header,
render the file block with 'binary file, diff omitted' instead of an
empty diff body.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Oversize diff truncation

**Files:**
- Modify: `global-skills/pipelight-run/tools/gen_diff_html.py`
- Modify: `global-skills/pipelight-run/tools/test_gen_diff_html.py`

- [ ] **Step 1: Write the failing test**

Append to `test_gen_diff_html.py`:

```python
class TestOversizeDiff(unittest.TestCase):
    def test_diff_exceeding_threshold_truncated(self):
        # Build a diff body that exceeds MAX_DIFF_BYTES (500KB default).
        # Create one big hunk with many add lines.
        header = (
            "diff --git a/big.txt b/big.txt\n"
            "--- a/big.txt\n"
            "+++ b/big.txt\n"
            "@@ -0,0 +1,1 @@\n"
        )
        # ~600KB of + lines
        big_body = "+x" * (600 * 1024 // 2) + "\n"
        fake_diff = header + big_body

        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "diff.txt").write_text("big.txt\n")
            (tmp / "base-ref.txt").write_text("origin/main\n")

            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                if cmd[:2] == ["git", "ls-files"]:
                    return SimpleNamespace(returncode=0, stdout="big.txt\n", stderr="")
                if cmd[:2] == ["git", "diff"]:
                    return SimpleNamespace(returncode=0, stdout=fake_diff, stderr="")
                return SimpleNamespace(returncode=0, stdout="", stderr="")

            with patch("subprocess.run", side_effect=fake_run):
                import importlib
                import gen_diff_html as mod
                importlib.reload(mod)
                rc = mod.main([
                    "--input", str(tmp / "diff.txt"),
                    "--base-ref-file", str(tmp / "base-ref.txt"),
                    "--output", str(tmp / "diff.html"),
                    "--cwd", str(tmp),
                ])
            self.assertEqual(rc, 0)
            html_out = (tmp / "diff.html").read_text()
            self.assertIn("diff too large", html_out)
            self.assertIn("KB, omitted", html_out)
            # The 600KB of "xxx...xxx" content must NOT appear in the HTML
            self.assertNotIn("x" * 10000, html_out)
```

- [ ] **Step 2: Run test to verify it fails**

```bash
python3 -m unittest test_gen_diff_html.TestOversizeDiff -v
```

Expected: FAIL — either test times out or the full 600KB body ends up in the HTML.

- [ ] **Step 3: Implement size-based truncation**

Edit `gen_diff_html.py`. Replace the tracked branch of `render_file_block` (from Task 7) with the version below — adds a size check after the binary check and before parsing hunks. The untracked fallthrough branch is unchanged.

```python
def render_file_block(path: str, anchor: str, base_ref: str, cwd: Path) -> tuple[str, str]:
    if is_tracked(path, base_ref, cwd):
        diff_text, _err = get_diff(path, base_ref, cwd)
        if is_binary_diff(diff_text):
            toc = (
                f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
                f'<span class="stat">binary</span></li>'
            )
            det = (
                f'<details id="{html.escape(anchor)}">'
                f'<summary><span class="path">{html.escape(path)}</span>'
                f' <span class="stat">binary</span></summary>'
                f'<div class="diff-body"><p>binary file, diff omitted</p></div>'
                f'</details>'
            )
            return toc, det
        diff_bytes = len(diff_text.encode("utf-8"))
        if diff_bytes > MAX_DIFF_BYTES:
            kb = diff_bytes // 1024
            toc = (
                f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
                f'<span class="stat">truncated</span></li>'
            )
            det = (
                f'<details id="{html.escape(anchor)}">'
                f'<summary><span class="path">{html.escape(path)}</span>'
                f' <span class="stat">truncated</span></summary>'
                f'<div class="diff-body"><p>diff too large ({kb} KB, omitted)</p></div>'
                f'</details>'
            )
            return toc, det
        hunks = parse_unified_diff(diff_text)
        add, dele = count_stats(hunks)
        body = render_tracked_body(path, hunks)
        toc = (
            f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
            f'<span class="stat">+{add} &minus;{dele}</span></li>'
        )
        det = (
            f'<details id="{html.escape(anchor)}">'
            f'<summary><span class="path">{html.escape(path)}</span>'
            f' <span class="stat">+{add} &minus;{dele}</span></summary>'
            f'{body}'
            f'</details>'
        )
        return toc, det
    # Untracked branch (from Task 6): name + line count only, no body.
    line_count = count_file_lines(path, cwd)
    toc = (
        f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
        f'<span class="stat badge-new">new</span></li>'
    )
    det = (
        f'<details id="{html.escape(anchor)}">'
        f'<summary><span class="path">{html.escape(path)}</span>'
        f' <span class="stat badge-new">+{line_count} lines (new file)</span></summary>'
        f'</details>'
    )
    return toc, det
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
python3 -m unittest test_gen_diff_html -v
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add global-skills/pipelight-run/tools/gen_diff_html.py global-skills/pipelight-run/tools/test_gen_diff_html.py
git commit -m "feat(tools): truncate oversize diffs above 500KB threshold

Per-file git diff outputs larger than MAX_DIFF_BYTES (500KB) are
replaced with a 'diff too large' placeholder. Prevents HTML from
becoming unreadable on machine-generated or vendored files.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Anchor collision regression test

**Files:**
- Modify: `global-skills/pipelight-run/tools/test_gen_diff_html.py`

- [ ] **Step 1: Write the test**

Anchor collision handling already exists in `render_html` (from Task 4), but it's untested. Add a dedicated test.

Append to `test_gen_diff_html.py`:

```python
class TestAnchorCollision(unittest.TestCase):
    def test_paths_with_same_slug_get_numeric_suffix(self):
        # Both paths slug to "f-a-b" — the second must become "f-a-b-2".
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "diff.txt").write_text("a/b.rs\na-b.rs\n")
            (tmp / "base-ref.txt").write_text("origin/main\n")

            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                # Both paths untracked to keep test simple
                if cmd[:2] == ["git", "ls-files"]:
                    return SimpleNamespace(returncode=1, stdout="", stderr="")
                return SimpleNamespace(returncode=0, stdout="", stderr="")

            with patch("subprocess.run", side_effect=fake_run):
                import importlib
                import gen_diff_html as mod
                importlib.reload(mod)
                rc = mod.main([
                    "--input", str(tmp / "diff.txt"),
                    "--base-ref-file", str(tmp / "base-ref.txt"),
                    "--output", str(tmp / "diff.html"),
                    "--cwd", str(tmp),
                ])
            self.assertEqual(rc, 0)
            html_out = (tmp / "diff.html").read_text()
            self.assertIn('id="f-a-b"', html_out)
            self.assertIn('id="f-a-b-2"', html_out)
```

- [ ] **Step 2: Run test to verify it passes**

```bash
python3 -m unittest test_gen_diff_html.TestAnchorCollision -v
```

Expected: PASS — collision handler from Task 4 already covers this.

- [ ] **Step 3: Commit**

```bash
git add global-skills/pipelight-run/tools/test_gen_diff_html.py
git commit -m "test(tools): regression test for anchor-slug collisions

Codifies the Task 4 dedup behavior: second path sharing a slug gets
-2 suffix.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: Inline CSS + header metadata

**Files:**
- Modify: `global-skills/pipelight-run/tools/gen_diff_html.py`
- Modify: `global-skills/pipelight-run/tools/test_gen_diff_html.py`

- [ ] **Step 1: Write the failing tests**

Append to `test_gen_diff_html.py`:

```python
class TestStylingAndHeader(unittest.TestCase):
    def _bootstrap(self, tmp: Path):
        (tmp / "diff.txt").write_text("")
        (tmp / "base-ref.txt").write_text("origin/main\n")

    def _run(self, tmp: Path):
        def fake_run(cmd, *a, **kw):
            from types import SimpleNamespace
            # Pretend we're in a git repo with a known HEAD
            if cmd[:3] == ["git", "rev-parse", "--abbrev-ref"]:
                return SimpleNamespace(returncode=0, stdout="feat/demo\n", stderr="")
            if cmd[:3] == ["git", "rev-parse", "--short"]:
                return SimpleNamespace(returncode=0, stdout="abc1234\n", stderr="")
            if cmd[:2] == ["git", "ls-files"]:
                return SimpleNamespace(returncode=0, stdout="", stderr="")
            return SimpleNamespace(returncode=0, stdout="", stderr="")
        with patch("subprocess.run", side_effect=fake_run):
            import importlib
            import gen_diff_html as mod
            importlib.reload(mod)
            return mod.main([
                "--input", str(tmp / "diff.txt"),
                "--base-ref-file", str(tmp / "base-ref.txt"),
                "--output", str(tmp / "diff.html"),
                "--cwd", str(tmp),
            ])

    def test_inline_css_present(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            self._bootstrap(tmp)
            self.assertEqual(self._run(tmp), 0)
            html_out = (tmp / "diff.html").read_text()
            # Substantive inline CSS — not just an empty <style> block
            self.assertIn("<style>", html_out)
            self.assertIn("#0d1117", html_out)  # page bg color
            self.assertIn(".line.add", html_out)
            self.assertIn(".line.del", html_out)
            self.assertIn(".hunk-header", html_out)

    def test_header_shows_branch_and_sha(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            self._bootstrap(tmp)
            self.assertEqual(self._run(tmp), 0)
            html_out = (tmp / "diff.html").read_text()
            self.assertIn("feat/demo", html_out)
            self.assertIn("abc1234", html_out)
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
python3 -m unittest test_gen_diff_html.TestStylingAndHeader -v
```

Expected: FAIL — current header does not include branch/sha and `<style>` is a placeholder comment.

- [ ] **Step 3: Implement inline CSS and header enrichment**

Edit `gen_diff_html.py`. Add a module-level CSS constant:

```python
CSS = """
:root {
  --bg: #0d1117;
  --fg: #c9d1d9;
  --add: #033a16;
  --del: #67060c;
  --ctx-border: #21262d;
  --hunk: #1f6feb;
  --muted: #8b949e;
}
body { background: var(--bg); color: var(--fg); font-family: ui-monospace, Menlo, Consolas, monospace; margin: 24px; }
h1 { margin-top: 0; font-size: 22px; }
h2 { font-size: 16px; border-bottom: 1px solid var(--ctx-border); padding-bottom: 4px; }
.meta { color: var(--muted); font-size: 13px; line-height: 1.6; margin-bottom: 16px; }
.meta code { background: #161b22; padding: 1px 6px; border-radius: 4px; }
.toc ul { list-style: none; padding-left: 0; }
.toc li { padding: 3px 0; }
.toc a { color: var(--fg); text-decoration: none; }
.toc a:hover { text-decoration: underline; }
.stat { color: var(--muted); font-size: 12px; margin-left: 8px; }
.badge-new { color: #3fb950; }
details { border: 1px solid var(--ctx-border); border-radius: 6px; margin: 8px 0; background: #161b22; }
summary { padding: 8px 12px; cursor: pointer; user-select: none; }
summary .path { font-weight: 600; }
.diff-body { border-top: 1px solid var(--ctx-border); }
.hunk-header { background: var(--hunk); color: white; padding: 4px 12px; font-size: 12px; }
pre.code { margin: 0; padding: 0; overflow-x: auto; }
pre.code code { display: block; }
.line { display: flex; padding: 0 12px; white-space: pre; }
.line .gutter { color: var(--muted); padding-right: 12px; user-select: none; }
.line.add { background: var(--add); }
.line.del { background: var(--del); }
.line.ctx { background: transparent; }
.line .content { flex: 1; }
"""
```

Add helpers to fetch branch + sha:

```python
def get_head_info(cwd: Path) -> tuple[str, str]:
    """Return (branch, short_sha). Empty strings if git fails."""
    rc1, branch, _ = git_output(["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd)
    rc2, sha, _ = git_output(["git", "rev-parse", "--short", "HEAD"], cwd)
    return (branch.strip() if rc1 == 0 else ""), (sha.strip() if rc2 == 0 else "")
```

Modify `render_html` to use CSS and include HEAD info in the header:

```python
def render_html(base_ref: str, paths: list[str], cwd: Path) -> str:
    now = datetime.now().astimezone().strftime("%Y-%m-%d %H:%M:%S %Z")
    branch, sha = get_head_info(cwd)
    head_label = f"{branch} @ {sha}" if branch and sha else (branch or sha or "(unknown)")

    seen = {}
    files = []
    for p in paths:
        slug = "f-" + re.sub(r"[^A-Za-z0-9]+", "-", p).strip("-").lower()
        seen[slug] = seen.get(slug, 0) + 1
        if seen[slug] > 1:
            slug = f"{slug}-{seen[slug]}"
        files.append((p, slug))

    toc_items = []
    file_blocks = []
    total_add, total_del = 0, 0
    for path, anchor in files:
        toc_li, det = render_file_block(path, anchor, base_ref, cwd)
        toc_items.append(toc_li)
        file_blocks.append(det)
        # Extract per-file stats from toc_li text if available — best-effort.
        m = re.search(r"\+(\d+) &minus;(\d+)", toc_li)
        if m:
            total_add += int(m.group(1))
            total_del += int(m.group(2))

    parts = [
        "<!DOCTYPE html>",
        '<html lang="zh-CN"><head><meta charset="utf-8">',
        f"<title>git-diff report — {html.escape(head_label)} vs {html.escape(base_ref)}</title>",
        f"<style>{CSS}</style>",
        "</head><body>",
        "<header>",
        "<h1>git-diff report</h1>",
        '<div class="meta">',
        f"<div>Base: <code>{html.escape(base_ref)}</code></div>",
        f"<div>HEAD: <code>{html.escape(head_label)}</code></div>",
        f"<div>Generated: {html.escape(now)}</div>",
        f"<div>Files changed: {len(files)} &middot; +{total_add} &minus;{total_del}</div>",
        "</div>",
        "</header>",
        '<section class="toc"><h2>Files</h2><ul>',
        *toc_items,
        "</ul></section>",
        '<section class="files">',
        *file_blocks,
        "</section>",
        "</body></html>",
    ]
    return "\n".join(parts)
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
python3 -m unittest test_gen_diff_html -v
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add global-skills/pipelight-run/tools/gen_diff_html.py global-skills/pipelight-run/tools/test_gen_diff_html.py
git commit -m "feat(tools): inline CSS and HEAD metadata in HTML header

Dark-palette inline CSS (no CDN, single-file distributable). Header now
shows current branch + short SHA via git rev-parse, plus aggregate
+add/-del totals computed from the TOC.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: `/pipelight-sync` — detect + install Pygments

**Files:**
- Modify: `.claude/skills/pipelight-sync/SKILL.md`

- [ ] **Step 1: Locate Step 2 in the sync skill**

Open `.claude/skills/pipelight-sync/SKILL.md` and find Step 2 ("Check Dev Environment") which lists `rustc --version`, `cargo --version`, etc.

- [ ] **Step 2: Add Python + Pygments checks to Step 2**

Append these checks to the tool list in Step 2:

```bash
python3 --version     # Python runtime (required by gen_diff_html.py)
python3 -c "import pygments, sys; sys.stdout.write(pygments.__version__)"   # Pygments
```

Update the "For each tool" policy paragraph to include the Python/Pygments handling:

```markdown
- **python3**:
  - **Installed** → `OK: Python 3.x.y`
  - **Not installed** → print install hint for the platform (`apt install python3` / `brew install python3`); do NOT auto-install (Python is typically pre-installed; auto-install is fragile cross-platform).

- **pygments**:
  - **Installed** → `OK: pygments x.y`
  - **Not installed** → auto-install with the ladder below. Abort the ladder on first success.
    1. `python3 -m pip install --user pygments`
    2. If step 1 fails with "externally-managed-environment" (PEP 668): `python3 -m pip install --user --break-system-packages pygments`
    3. If step 2 also fails: print a red error telling the user to install manually (e.g. `apt install python3-pygments` / `brew install pygments`) and continue the sync — pygments is only blocking when the user later runs `pipelight run --git-diff-from-remote-branch`.
```

Also add the two new rows to the Step 4 report table:

```markdown
Environment:
  rustc        OK 1.94.1
  cargo        OK 1.94.1
  docker       OK 27.x.x (daemon running)
  git          OK 2.x.x
  claude       OK 1.x.x (optional)
  python3      OK 3.x.y
  pygments     OK x.y   — or INSTALLED (via pip --user) — or FAILED (manual install required)
```

- [ ] **Step 3: Manual spot-check (no unit test — skill is prose)**

Re-read the edited section to verify:
- Python check comes AFTER `claude --version`
- Pygments auto-install ladder is exactly 3 steps with the documented fallback
- Report table has the two new rows

- [ ] **Step 4: Commit**

```bash
git add .claude/skills/pipelight-sync/SKILL.md
git commit -m "feat(sync): detect python3 + pygments, auto-install pygments

pipelight-sync now checks for python3 and pygments as part of the dev
environment. Pygments missing triggers an auto-install ladder:
pip --user → pip --user --break-system-packages → manual hint. Never
uses sudo; never creates a venv. Required for the new gen_diff_html.py
tool that runs on --git-diff-from-remote-branch.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 12: `pipelight-run` SKILL.md — HTML branch in `git_diff_report` flow

**Files:**
- Modify: `global-skills/pipelight-run/SKILL.md`
- Sync: `cp -r global-skills/pipelight-run/ ~/.claude/skills/pipelight-run/`

- [ ] **Step 1: Update the `git_diff_report` detailed flow**

Open `global-skills/pipelight-run/SKILL.md`, find the `#### git_diff_report 详细流程` section (near line 449).

Replace the numbered steps with:

```markdown
1. 从 `on_failure.context_paths` 读文件清单：
   - `pipelight-misc/git-diff-report/diff.txt` — 单一汇总文档，当前分支所有变更文件的去重路径列表
   - `pipelight-misc/git-diff-report/base-ref.txt`（**可选**）— 当用户传了 `--git-diff-from-remote-branch=<ref>` 时出现；单行写入本次使用的 base ref。
2. step stdout 仍会打印分类统计（unstaged / staged / untracked / branch-ahead）供人类阅读。
3. **终端打印 markdown 清单**（始终执行，无论 base-ref.txt 是否存在）：

```markdown
### git-diff: 5 unique file(s) changed on current branch

- unstaged: 2
- staged: 1
- untracked: 0
- branch-ahead (vs origin/main): 2

**Files:**
- src/foo.rs
- src/bar.rs
- ...
```

4. **HTML 报告生成**（**仅当 context_paths 含 base-ref.txt 时**执行）：

   运行 bundled 工具生成一份独立 HTML review artifact：

```bash
python3 ~/.claude/skills/pipelight-run/tools/gen_diff_html.py \
    --input pipelight-misc/git-diff-report/diff.txt \
    --base-ref-file pipelight-misc/git-diff-report/base-ref.txt \
    --output pipelight-misc/git-diff-report/diff.html \
    --cwd <repo-root>
```

   - 成功（退出 0）→ 打印一行提示，如 `HTML report: pipelight-misc/git-diff-report/diff.html (open in browser for review)`
   - 失败（退出非 0）→ 把 stderr 打印到终端；注明 `HTML report failed; Markdown list above is the complete output`；**不 retry pipelight、不 abort pipeline**（HTML 是人工 review 附加品，不影响 CI 判定）
   - Pygments 未安装导致的失败 → 提示用户跑 `/pipelight-sync` 或手动 `python3 -m pip install --user pygments`

5. **不修代码、不 retry**，打印完继续下一 step 的分发。
```

- [ ] **Step 2: Sync to local**

```bash
cp -r global-skills/pipelight-run/ ~/.claude/skills/pipelight-run/
```

Verify the tool is now at `~/.claude/skills/pipelight-run/tools/gen_diff_html.py`:

```bash
ls ~/.claude/skills/pipelight-run/tools/
```

Expected: `gen_diff_html.py`, `test_gen_diff_html.py`.

- [ ] **Step 3: Run the py tool tests from the synced location as a smoke test**

```bash
cd ~/.claude/skills/pipelight-run/tools
python3 -m unittest test_gen_diff_html -v
```

Expected: all tests pass from the synced location (no path-specific assumptions in tests).

- [ ] **Step 4: Commit**

```bash
git add global-skills/pipelight-run/SKILL.md
git commit -m "docs(skill): document HTML branch in git_diff_report flow

git_diff_report now has two possible actions:
- markdown terminal print (always, unchanged)
- HTML artifact generation via gen_diff_html.py (only when
  context_paths contains base-ref.txt, i.e. user passed
  --git-diff-from-remote-branch)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 13: End-to-end verification on rc project

**Files:** none modified — this is a manual verification step.

- [ ] **Step 1: Run the full flow**

```bash
cd ~/workshop/rc
pipelight run -d ~/workshop/rc --git-diff-from-remote-branch=origin/hotfix --output json
```

Capture the JSON output and look for the git-diff step's `on_failure`:

```jsonc
{
  "command": "git_diff_command",
  "action": "git_diff_report",
  "context_paths": [
    "pipelight-misc/git-diff-report/diff.txt",
    "pipelight-misc/git-diff-report/base-ref.txt"
  ]
}
```

- [ ] **Step 2: Verify both sidecar files exist**

```bash
cat ~/workshop/rc/pipelight-misc/git-diff-report/diff.txt | head
cat ~/workshop/rc/pipelight-misc/git-diff-report/base-ref.txt
```

Expected: `diff.txt` has paths (one per line); `base-ref.txt` contains exactly `origin/hotfix`.

- [ ] **Step 3: Invoke the py tool manually**

```bash
python3 ~/.claude/skills/pipelight-run/tools/gen_diff_html.py \
  --input  ~/workshop/rc/pipelight-misc/git-diff-report/diff.txt \
  --base-ref-file ~/workshop/rc/pipelight-misc/git-diff-report/base-ref.txt \
  --output ~/workshop/rc/pipelight-misc/git-diff-report/diff.html \
  --cwd    ~/workshop/rc
```

- [ ] **Step 4: Open in browser and check**

```bash
xdg-open ~/workshop/rc/pipelight-misc/git-diff-report/diff.html  # Linux
# or: open ...  on macOS
```

Visual checks:
- [ ] Header shows base `origin/hotfix`, current branch + short SHA, timestamp, file count + aggregate stats
- [ ] TOC lists every file from `diff.txt`
- [ ] Clicking a TOC entry jumps to its `<details>` via anchor
- [ ] Tracked files show `@@` hunk headers, `+`/`-`/context lines with colored backgrounds, syntax highlighting for known extensions
- [ ] Untracked files (if any) show no body, just `+N lines (new file)` in summary
- [ ] Binary files (if any) show "binary file, diff omitted"
- [ ] Dark theme rendering — no white flashes, no CDN requests (check network tab is idle)

- [ ] **Step 5: Document findings (if issues)**

If any of the visual checks fail, file a fixup task. If all pass, no action needed.

---

## Self-review

Before handing off:

- [ ] All tasks have explicit file paths
- [ ] All code steps include actual code blocks
- [ ] All tests include full assertion bodies
- [ ] Commit messages cover the WHY
- [ ] Task ordering is TDD-consistent (test → run-fail → impl → run-pass → commit)
- [ ] Spec coverage: every section in `2026-04-22-git-diff-html-report-design.md` is covered
  - §1 Summary: Tasks 1-12 together
  - §2 Motivation / §3 Non-goals: no code tasks needed
  - §4 Key decisions 1 (gate): Tasks 1, 2
  - §4 Key decisions 2 (sidecar): Task 1
  - §4 Key decisions 3 (no new variant): Task 2 (doc only)
  - §4 Key decisions 4 (py tool ownership): Tasks 3-10
  - §4 Key decisions 5 (Pygments): Tasks 5, 11
  - §4 Key decisions 6 (py tool semantics): Task 3
  - §4 Key decisions 7 (output path): Task 13 manual verification
  - §4 Key decisions 8 (untracked policy): Task 6
  - §4 Key decisions 9 (layout): Tasks 4, 10
  - §5 Architecture: Tasks 1-12
  - §6 Components: §§ matches tasks 1-12
  - §7 Data contracts: Tasks 1, 2, 3
  - §8 HTML structure: Tasks 4, 6, 7, 8, 10
  - §9 Error handling: Tasks 3 (tool errors), 7 (binary), 8 (oversize), 12 (LLM side)
  - §10 Testing strategy: Tasks 1-10 + Task 13 manual E2E

No gaps identified. Ready for subagent-driven execution.
