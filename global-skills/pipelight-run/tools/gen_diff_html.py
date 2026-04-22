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
