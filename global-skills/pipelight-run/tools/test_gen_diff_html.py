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


if __name__ == "__main__":
    unittest.main()
