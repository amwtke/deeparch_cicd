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
            self.assertNotIn("<li>", html)


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


if __name__ == "__main__":
    unittest.main()
