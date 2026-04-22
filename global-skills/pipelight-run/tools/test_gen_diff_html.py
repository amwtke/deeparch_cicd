"""Tests for gen_diff_html. Run with: python3 -m unittest test_gen_diff_html"""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch  # noqa: F401

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
            self.assertNotIn("<li><a", html)  # no TOC entry <li>s (CSS .toc li is fine)


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
                return SimpleNamespace(returncode=1, stdout="", stderr="")

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


    def test_git_diff_failure_renders_error_block(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            self._make_tmp(tmp, ["src/broken.py"])
            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                if cmd[:2] == ["git", "ls-files"]:
                    return SimpleNamespace(returncode=0, stdout="src/broken.py\n", stderr="")
                if cmd[:2] == ["git", "diff"]:
                    return SimpleNamespace(returncode=128, stdout="", stderr="fatal: bad revision\n")
                return SimpleNamespace(returncode=1, stdout="", stderr="")

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
            self.assertEqual(rc, 0, "git diff failure should be non-fatal")
            html_out = (tmp / "diff.html").read_text()
            self.assertIn('class="error"', html_out)
            self.assertIn("git diff failed", html_out)
            self.assertIn("fatal: bad revision", html_out)


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
                return SimpleNamespace(returncode=1, stdout="", stderr="")

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
                return SimpleNamespace(returncode=1, stdout="", stderr="")

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
                # default rc=1: cat-file, cat-file -e, or any other command →
                # "not found". Ensures is_tracked's base_ref check (from Task 4 fix)
                # correctly falls through to the untracked branch.
                return SimpleNamespace(returncode=1, stdout="", stderr="")

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
                return SimpleNamespace(returncode=1, stdout="", stderr="")

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
                return SimpleNamespace(returncode=1, stdout="", stderr="")

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

    def test_diff_below_threshold_renders_normally(self):
        # A small diff must NOT trigger the oversize branch — hunks render normally.
        fake_diff = (
            "diff --git a/small.txt b/small.txt\n"
            "--- a/small.txt\n"
            "+++ b/small.txt\n"
            "@@ -1,1 +1,1 @@\n"
            "-old\n"
            "+new\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "diff.txt").write_text("small.txt\n")
            (tmp / "base-ref.txt").write_text("origin/main\n")

            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                if cmd[:2] == ["git", "ls-files"]:
                    return SimpleNamespace(returncode=0, stdout="small.txt\n", stderr="")
                if cmd[:2] == ["git", "diff"]:
                    return SimpleNamespace(returncode=0, stdout=fake_diff, stderr="")
                return SimpleNamespace(returncode=1, stdout="", stderr="")

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
            self.assertNotIn("diff too large", html_out)
            self.assertIn("@@ -1,1 +1,1 @@", html_out)
            self.assertIn('class="line add"', html_out)
            self.assertIn('class="line del"', html_out)


class TestAnchorCollision(unittest.TestCase):
    def test_paths_with_same_slug_get_numeric_suffix(self):
        # _make_anchor replaces all non-alphanumeric chars with "-":
        #   "a/b.rs"  → "f-a-b-rs"
        #   "a-b.rs"  → "f-a-b-rs"   (collision!)
        # The second must receive the dedup suffix → "f-a-b-rs-2".
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "diff.txt").write_text("a/b.rs\na-b.rs\n")
            (tmp / "base-ref.txt").write_text("origin/main\n")

            def fake_run(cmd, *a, **kw):
                from types import SimpleNamespace
                # Both paths untracked to keep test simple
                if cmd[:2] == ["git", "ls-files"]:
                    return SimpleNamespace(returncode=1, stdout="", stderr="")
                return SimpleNamespace(returncode=1, stdout="", stderr="")

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
            self.assertIn('id="f-a-b-rs"', html_out)
            self.assertIn('id="f-a-b-rs-2"', html_out)


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


if __name__ == "__main__":
    unittest.main()
