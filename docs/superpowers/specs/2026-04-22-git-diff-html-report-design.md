# git-diff HTML report — design spec

Date: 2026-04-22
Author: brainstorming session with amwtke
Status: approved, ready for implementation planning

## Summary

Extend the existing `git_diff_report` callback action with an HTML-rendered,
self-contained diff report for post-run human review. The HTML path is
**gated on the `--git-diff-from-remote-branch=<ref>` CLI flag** — when the
flag is absent, behavior is unchanged. No new `CallbackCommand` or
`CallbackCommandAction` variants are introduced; we enrich what the action
already does.

## Motivation

Today the `git_diff_report` action reads
`pipelight-misc/git-diff-report/diff.txt` (a deduplicated file-path list)
and prints a Markdown summary to the terminal. That summary is ephemeral —
once the terminal buffer scrolls, the reviewer has to re-run the pipeline to
see the list again, and even then they only get filenames, not the actual
changes.

Feature-branch reviews (the use case for `--git-diff-from-remote-branch`)
often span dozens of files; a human reviewer needs to see *what* changed,
not just *which* files changed. A single self-contained HTML file — similar
to GitHub's PR "Files changed" tab — is the cheapest way to persist a
reviewable artifact without pushing the branch and cutting a PR.

## Non-goals

- No new callback command or action variant — we extend `git_diff_report`.
- No HTML when `--git-diff-from-remote-branch` is absent (workflow for
  local WIP review is already served by the markdown terminal print).
- No server, no watch mode, no index-of-HTMLs for historical runs — each
  run overwrites `diff.html`.
- No configurable style / theme — one hard-coded dark palette.
- No CLI flag to toggle HTML independently; it is implied by
  `--git-diff-from-remote-branch`.

## Key decisions

1. **Gate**: HTML is produced **iff** `--git-diff-from-remote-branch=<ref>`
   was passed (equivalently: `run_state.git_diff_base` is `Some`). The base
   ref used by the HTML report is that flag's value — no other source.
2. **Contract carrier**: a sidecar file `base-ref.txt` written by the
   git-diff shell step alongside `diff.txt`. The existence of this file in
   `on_failure.context_paths` is the signal to the LLM to generate HTML.
3. **No new callback variant**: `git_diff_report` keeps its name; its
   behavior is "print markdown list; additionally, if `base-ref.txt` is in
   context, generate HTML via py tool".
4. **Python tool ownership**: `global-skills/pipelight-run/tools/gen_diff_html.py`
   (versioned with the skill, auto-synced to
   `~/.claude/skills/pipelight-run/tools/` by `/pipelight-sync`).
5. **Pygments required**: the py tool hard-depends on `pygments` for
   syntax highlighting; detection & auto-install are added to
   `/pipelight-sync` (it already auto-installs rust via rustup).
6. **Py tool semantics**: self-contained CLI. Shells out to
   `git diff <base> -- <file>` itself per path in `diff.txt`. No coupling
   with the git-diff shell step beyond reading the two files.
7. **Output**: `pipelight-misc/git-diff-report/diff.html` (same dir as
   `diff.txt`), single HTML with inlined CSS — no assets, no CDN.
8. **Untracked files**: listed by name + new-line count only. No diff body
   rendered (per user directive — untracked files are typically generated
   artifacts / new source; a reviewer wants to know they exist, not read
   the whole file).
9. **Layout**: one `<details>` per file (default folded) + top-level TOC,
   so opening the file is fast regardless of diff volume.

## Architecture

```
CLI: pipelight run --git-diff-from-remote-branch=origin/main ...
        ↓ persists to run_state.git_diff_base = Some("origin/main")
GitDiffStep::with_base_ref(Some("origin/main"))
        ↓ shell script additionally writes base-ref.txt
pipelight-misc/git-diff-report/
  ├── diff.txt         (existing: all changed file paths, deduplicated)
  └── base-ref.txt     (NEW: single line, base ref string; only when base_ref=Some)
        ↓ step completes; JSON on_failure.context_paths has 1 or 2 files
        ↓
LLM dispatches on action=git_diff_report:
  1. read diff.txt → print Markdown file list (UNCHANGED behavior)
  2. check if context_paths includes base-ref.txt
     ├── yes → run py tool to generate diff.html
     └── no  → done (current behavior preserved)
        ↓ (only when yes)
python ~/.claude/skills/pipelight-run/tools/gen_diff_html.py \
    --input  pipelight-misc/git-diff-report/diff.txt \
    --base-ref-file pipelight-misc/git-diff-report/base-ref.txt \
    --output pipelight-misc/git-diff-report/diff.html
        ↓
Py tool: reads base ref, iterates diff.txt:
  - tracked file  → git diff <base> -- <file> → pygments highlight → <details> block
  - untracked     → count lines → "<details>" with empty body + "+N lines (new file)"
  assembles single HTML with TOC, inline CSS
```

## Components

### New

| Path | Purpose |
|---|---|
| `global-skills/pipelight-run/tools/gen_diff_html.py` | The CLI tool the LLM invokes |
| `global-skills/pipelight-run/tools/test_gen_diff_html.py` | Unit tests, `python3 -m unittest` runnable |

### Modified

| Path | Change |
|---|---|
| `src/ci/pipeline_builder/base/git_diff_step.rs` | shell: when `base_ref=Some`, also write `base-ref.txt` (after `BRANCH_AHEAD_ERR=0` guard). `exception_mapping()`: when `base_ref=Some`, `context_paths` includes both `diff.txt` and `base-ref.txt`. |
| `src/ci/callback/action.rs` | doc comment on `GitDiffReport` updated to mention optional HTML generation |
| `global-skills/pipelight-run/SKILL.md` | `git_diff_report` detailed flow section expanded with the HTML branch |
| `.claude/skills/pipelight-sync/SKILL.md` | Step 2 dev-environment checks add `python3 --version` and `python3 -c "import pygments"` with auto-install ladder |

### Unchanged

- `src/ci/callback/command.rs` — no new `CallbackCommand` variant
- `src/ci/callback/action.rs` — no new variant (only doc comment change)
- `src/cli/mod.rs` — the `--git-diff-from-remote-branch` flag already exists; no new flag
- `src/run_state/mod.rs` — `git_diff_base` field already exists

## Data contracts

### `base-ref.txt` format

- Single line, ASCII only, characters restricted to `[A-Za-z0-9/_.-]` (same
  whitelist `with_base_ref`'s `debug_assert!` already enforces at step
  construction time)
- Newline tolerance: py tool calls `.strip()` on read
- Absent when `base_ref = None` (not an empty file — simply does not exist)

### `on_failure.context_paths` shape

```jsonc
// base_ref = None — UNCHANGED
{ "context_paths": ["pipelight-misc/git-diff-report/diff.txt"] }

// base_ref = Some — new: 2 paths
{
  "context_paths": [
    "pipelight-misc/git-diff-report/diff.txt",
    "pipelight-misc/git-diff-report/base-ref.txt"
  ]
}
```

### Py tool CLI

```
python gen_diff_html.py \
  --input <path>             # diff.txt (REQUIRED)
  --base-ref-file <path>     # base-ref.txt (REQUIRED)
  --output <path>            # diff.html (REQUIRED)
  [--cwd <repo-root>]        # git operations run here; default = current CWD
```

Path resolution: `--input`, `--base-ref-file`, `--output` are passed to
Python's `open()` verbatim. Callers may use absolute paths or paths
relative to the process CWD. The py tool does **not** resolve them
relative to `--cwd` (which only scopes git operations).

Exit codes:
- `0` — success
- `1` — user error (missing/invalid inputs, unsafe base ref, pygments missing)
- `2` — internal error (should not happen in normal flow)

### Pygments auto-install ladder (in `/pipelight-sync`)

```
1.  pip install --user pygments
    ↓ failed (e.g. PEP 668 externally-managed env)
2.  pip install --user --break-system-packages pygments
    ↓ still failed
3.  STOP — print red error telling user to install manually
    (e.g. `apt install python3-pygments` / `brew install pygments`)
```

Never uses `sudo`; never creates a venv; py tool itself does NOT auto-install at runtime (only reports the missing import).

## HTML structure

### Skeleton

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <title>git-diff report — <branch> vs <base></title>
  <style>/* inline — single-file distributable */</style>
</head>
<body>
  <header>
    <h1>git-diff report</h1>
    <div class="meta">
      <div>Base: <code>origin/main</code></div>
      <div>HEAD: <code>feat/xxx @ abc1234</code></div>
      <div>Generated: 2026-04-22 15:30:12 CST</div><!-- local tz, via `datetime.now().astimezone()` + `%Z` -->
      <div>Files changed: 12 &middot; +450 &minus;120</div>
    </div>
  </header>

  <section class="toc">
    <h2>Files</h2>
    <ul>
      <li><a href="#f-src-foo-rs">src/foo.rs</a> <span class="stat">+45 −12</span></li>
      <li><a href="#f-new-md">docs/new.md</a> <span class="stat badge-new">new</span></li>
    </ul>
  </section>

  <section class="files">
    <details id="f-src-foo-rs">
      <summary>
        <span class="path">src/foo.rs</span>
        <span class="stat">+45 −12</span>
      </summary>
      <div class="diff-body">
        <div class="hunk">
          <div class="hunk-header">@@ -42,7 +42,9 @@ fn foo() {</div>
          <pre class="code"><code class="language-rust">…pygments-highlighted lines…</code></pre>
        </div>
      </div>
    </details>

    <details id="f-new-md">
      <summary>
        <span class="path">docs/new.md</span>
        <span class="stat badge-new">+120 lines (new file)</span>
      </summary>
      <!-- no body — untracked files are name/size only per spec -->
    </details>
  </section>
</body>
</html>
```

### Styling basics

- Dark palette, GitHub-dark-diff inspired:
  - page background `#0d1117`
  - added line bg `#033a16`
  - removed line bg `#67060c`
  - hunk-header bg `#1f6feb` (blue accent)
- Font stack: `ui-monospace, Menlo, Consolas, monospace`
- All CSS inlined in `<style>` — no external assets, no CDN references

### Anchor ID rules

- `id = "f-" + slug`, where `slug = re.sub(r'[^A-Za-z0-9]+', '-', path).strip('-').lower()`
- Collision handling: maintain a `set()` while iterating; on conflict, append `-2`, `-3`, …
  - Example: `a/b.rs` → slug `a-b` → `f-a-b`; a separate file `a-b.rs` also slugs to `a-b` → `f-a-b-2`

### File block variants

| File type | `<details>` | TOC stat | Body |
|---|---|---|---|
| tracked (modified/staged/unstaged/branch-ahead) | yes, folded | `+N −M` | hunks with pygments highlight |
| untracked (new file) | yes, folded | `new` (badge) | **empty** — only summary |
| binary (git reports `Binary files differ`) | yes, folded | `binary` | "binary file, diff omitted" |
| oversized diff (>500 KB raw output) | yes, folded | `+N −M (truncated)` | "diff too large (N KB), omitted" |

Threshold `500 KB` is a fixed constant in py tool; not a CLI knob.

## Error handling

### git-diff step (Rust / shell)

| Scenario | Handling | Impact on existing behavior |
|---|---|---|
| `base_ref = None` | Skip `base-ref.txt`; `context_paths = [diff.txt]` only | None — existing markdown-only path |
| `base_ref = Some` and ref invalid | Existing logic: shell exits 2 → `git_diff_base_not_found` → `RuntimeError` | None |
| `base_ref = Some` and ref valid | Write `base-ref.txt`; `context_paths = [diff.txt, base-ref.txt]` | **NEW branch** |
| Writing `base-ref.txt` fails (IO error) | Propagates as non-zero step exit; user diagnoses via stderr | step-level failure — same shape as any shell error |

Ordering guarantee: `base-ref.txt` is written **after** the `BRANCH_AHEAD_ERR` check passes (i.e. only on success path), so we never leave a stale sidecar when exit code is 2.

### Py tool

| Scenario | Exit | Behavior |
|---|---|---|
| `--input` missing | 1 | stderr `diff.txt not found at <path>` |
| `--base-ref-file` missing | 1 | stderr `base-ref.txt not found at <path>` |
| `base-ref.txt` fails `[A-Za-z0-9/_.-]` whitelist | 1 | stderr `unsafe base ref '<x>' — may be tampered` |
| `import pygments` fails | 1 | stderr `Pygments not installed. Run: python3 -m pip install --user pygments` |
| `git diff <base> -- <file>` returns non-zero for one file | non-fatal | render `<div class="error">git diff failed: <stderr>…</div>` in that file's body; continue with remaining files |
| File is binary | non-fatal | render "binary file, diff omitted" |
| Diff output > 500 KB | non-fatal | render "diff too large (N KB), omitted" |
| Pygments lexer not found for extension | non-fatal fallback | fall back to `TextLexer` — lines still colored by `+/-` prefix |

### LLM side (SKILL.md guidance)

```
if context_paths contains base-ref.txt:
  run py tool
  if exit != 0:
    print stderr to terminal
    note: "HTML report failed; Markdown list already printed above"
    do NOT retry pipelight (git_diff_report is non-retry)
    do NOT abort pipeline (HTML is a human-review side artifact)
else:
  skip HTML; Markdown list is the complete output (existing behavior)
```

## Testing strategy

### Rust unit tests (in `src/ci/pipeline_builder/base/git_diff_step.rs`)

| Test | Assertion |
|---|---|
| `test_script_writes_base_ref_file_when_some` | `with_base_ref(Some("origin/main"))` → shell contains `echo "$BASE" > "$REPORT_DIR/base-ref.txt"` |
| `test_script_does_not_write_base_ref_file_when_none` | `new()` → shell does **not** mention `base-ref.txt` |
| `test_base_ref_file_written_after_branch_ahead_err_guard` | sidecar write appears in shell **after** the `BRANCH_AHEAD_ERR=1` → `exit 2` line (so stale sidecars don't linger) |
| `test_context_paths_includes_base_ref_file_when_some` | `exception_mapping()` entry for `git_diff_changes_found` → `context_paths.len() == 2` when `base_ref=Some` |
| `test_context_paths_one_path_when_none` | Same entry → `context_paths.len() == 1` when `base_ref=None` (regression guard for existing behavior) |

### Py tool unit tests (`test_gen_diff_html.py`, runnable via `python3 -m unittest`)

| Test | Assertion |
|---|---|
| `test_empty_input_exits_0` | empty `diff.txt` → HTML with empty TOC, exit 0 |
| `test_single_tracked_file_renders` | mocked `git diff` output → HTML contains `<details>`, hunk header, colored `+/-` lines |
| `test_untracked_file_no_body` | path listed only in diff.txt but absent in `git diff` → `<details>` with no body, summary contains `(new file)` and line count |
| `test_binary_file_omitted` | `git diff` output contains `Binary files differ` → "binary file, diff omitted" |
| `test_large_diff_truncated` | mock > 500 KB output → "diff too large" rendered, full diff not in HTML |
| `test_pygments_missing_exits_1` | `sys.modules['pygments'] = None` → exit 1 + stderr contains `pip install` |
| `test_unsafe_base_ref_rejected` | `base-ref.txt` content `; rm -rf /` → exit 1 |
| `test_anchor_collision` | two paths slug-collide → second gets `-2` suffix |
| `test_missing_input_file_exits_1` | `--input` path does not exist → exit 1 |
| `test_missing_base_ref_file_exits_1` | `--base-ref-file` path does not exist → exit 1 |

Git invocation is mocked via `unittest.mock.patch('subprocess.run')` — no temp repo fixture needed.

### End-to-end (manual, post-implementation)

```bash
cd ~/workshop/rc
pipelight run -d ~/workshop/rc --git-diff-from-remote-branch=origin/hotfix
```

Expected:
- `pipelight-misc/git-diff-report/diff.txt` generated
- `pipelight-misc/git-diff-report/base-ref.txt` generated, content `origin/hotfix`
- LLM prints Markdown file list (unchanged)
- LLM invokes py tool; `diff.html` appears
- Open `diff.html` in browser → TOC + per-file `<details>` (folded), Pygments-highlighted hunks, untracked files summary-only

## Task breakdown (for writing-plans)

Approximately 8 TDD tasks in this order:

1. **Rust**: shell script emits `base-ref.txt` when `base_ref=Some` (+ tests). Ordering guarantee: after BRANCH_AHEAD_ERR guard.
2. **Rust**: `exception_mapping()` includes `base-ref.txt` in `context_paths` conditionally (+ tests).
3. **Py**: minimal skeleton — CLI parsing, pygments import check, empty input → empty HTML.
4. **Py**: tracked file diff rendering — shell out to `git diff`, parse hunks, pygments highlight, inline CSS.
5. **Py**: untracked / binary / oversize branches (+ tests for each).
6. **Py**: TOC + anchor collision handling + header/meta block.
7. **`.claude/skills/pipelight-sync/SKILL.md`**: add python3 + pygments detection, install ladder in Step 2.
8. **`global-skills/pipelight-run/SKILL.md`**: expand `git_diff_report` detailed flow with HTML branch; run `/pipelight-sync`-equivalent sync so local `~/.claude/skills/pipelight-run/` gets the py tool.

## Open questions

None outstanding at spec-approval time.

## References

- Prior spec: `docs/superpowers/specs/2026-04-21-git-diff-from-remote-branch-design.md`
- Prior plan: `docs/superpowers/plans/2026-04-22-git-diff-from-remote-branch.md`
- Existing action doc: `src/ci/callback/action.rs:23-26` (`GitDiffReport`)
- Existing skill flow: `global-skills/pipelight-run/SKILL.md` §`git_diff_report` 详细流程
