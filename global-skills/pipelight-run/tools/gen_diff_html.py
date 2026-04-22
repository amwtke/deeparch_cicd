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
from __future__ import annotations

import argparse
import html
import re
import subprocess
import sys
from datetime import datetime
from pathlib import Path

from pygments import highlight
from pygments.formatters import HtmlFormatter
from pygments.lexers import get_lexer_by_name, get_lexer_for_filename
from pygments.util import ClassNotFound

# ASCII whitelist mirroring the Rust side's debug_assert!.
SAFE_REF_RE = re.compile(r"^[A-Za-z0-9/_.-]+$")

# If `git diff` emits more than this many bytes for a single file, we
# replace the body with a "too large" placeholder.
MAX_DIFF_BYTES = 500 * 1024


def die(msg: str, code: int = 1) -> None:
    print(msg, file=sys.stderr)
    sys.exit(code)


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


def _make_anchor(path: str) -> str:
    """Convert a file path to a stable HTML anchor id."""
    return "f-" + re.sub(r"[^A-Za-z0-9]+", "-", path).strip("-").lower()


def git_output(cmd: list[str], cwd: Path) -> tuple[int, str, str]:
    """Run a git command; return (returncode, stdout, stderr). Never raises."""
    r = subprocess.run(cmd, cwd=str(cwd), capture_output=True, text=True)
    return r.returncode, r.stdout, r.stderr


def is_tracked(path: str, base_ref: str, cwd: Path) -> bool:
    """True if this path has a diffable state — present at HEAD/index or at base_ref.

    Files truly absent from both (pure untracked / net-new) return False.
    """
    rc, _, _ = git_output(["git", "ls-files", "--error-unmatch", "--", path], cwd)
    if rc == 0:
        return True
    rc, _, _ = git_output(["git", "cat-file", "-e", f"{base_ref}:{path}"], cwd)
    return rc == 0


def get_diff(path: str, base_ref: str, cwd: Path) -> tuple[int, str, str]:
    """Return (rc, stdout, stderr) from `git diff <base> -- <path>`."""
    return git_output(["git", "diff", base_ref, "--", path], cwd)


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
        if line.startswith("\\"):
            # "\ No newline at end of file" sentinel — skip
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


def pick_lexer(path: str):
    try:
        return get_lexer_for_filename(path, stripnl=False)
    except ClassNotFound:
        # Content-based guess_lexer deferred — TextLexer is safer (never
        # mis-highlights) and the file list is already path-oriented.
        return get_lexer_by_name("text", stripnl=False)


def highlight_content(content: str, lexer, formatter) -> str:
    """Highlight a single logical line. Returns inline HTML without wrapping <pre>."""
    return highlight(content, lexer, formatter).rstrip("\n")


def render_tracked_body(path: str, hunks: list[dict]) -> str:
    lexer = pick_lexer(path)
    formatter = HtmlFormatter(nowrap=True)
    out = ['<div class="diff-body">']
    for h in hunks:
        out.append('<div class="hunk">')
        out.append(f'<div class="hunk-header">{html.escape(h["header"])}</div>')
        out.append('<pre class="code"><code>')
        for kind, content in h["lines"]:
            prefix = {"add": "+", "del": "-", "ctx": " "}[kind]
            highlighted = highlight_content(content, lexer, formatter)
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


def render_file_block(path: str, anchor: str, base_ref: str, cwd: Path) -> tuple[str, str]:
    """Return (toc_li_html, details_block_html) for one file."""
    if is_tracked(path, base_ref, cwd):
        rc, diff_text, err = get_diff(path, base_ref, cwd)
        if rc != 0:
            toc = (
                f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a> '
                f'<span class="stat">error</span></li>'
            )
            det = (
                f'<details id="{html.escape(anchor)}">'
                f'<summary><span class="path">{html.escape(path)}</span>'
                f' <span class="stat">error</span></summary>'
                f'<div class="diff-body"><div class="error">git diff failed: {html.escape(err.strip() or "(no stderr)")}</div></div>'
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
    # Untracked, binary, oversize branches come in later tasks. For now,
    # fall through to an untracked-style placeholder so Task 4 is self-consistent.
    toc = f'<li><a href="#{html.escape(anchor)}">{html.escape(path)}</a></li>'
    det = (
        f'<details id="{html.escape(anchor)}">'
        f'<summary><span class="path">{html.escape(path)}</span></summary>'
        f'</details>'
    )
    return toc, det


def render_html(base_ref: str, paths: list[str], cwd: Path) -> str:
    now = datetime.now().astimezone().strftime("%Y-%m-%d %H:%M:%S %Z")
    # Collision dedup: suffix -2, -3, … on repeated slug.
    seen = {}
    files = []
    for p in paths:
        slug = _make_anchor(p)
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


def main(argv=None) -> int:
    args = parse_args(argv)
    input_path = Path(args.input)
    base_ref_path = Path(args.base_ref_file)
    output_path = Path(args.output)
    cwd = Path(args.cwd)

    base_ref = read_base_ref(base_ref_path)
    paths = read_diff_paths(input_path)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(render_html(base_ref, paths, cwd), encoding="utf-8")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except ImportError as e:
        if "pygments" in str(e).lower():
            die("Pygments not installed. Run: python3 -m pip install --user pygments", code=1)
        raise
