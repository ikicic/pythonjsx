"""Integration tests: compile and run .px fixtures, compare stdout to .txt."""

import difflib
import subprocess
import sys
import unittest
from pathlib import Path

FIXTURES_DIR = Path(__file__).parent / "e2e"
PROJECT_ROOT = Path(__file__).parent.parent.parent


def _make_test(px_file: Path, txt_file: Path):
    def test(self: unittest.TestCase):
        result = subprocess.run(
            [sys.executable, "-m", "pythonjsx", "run", str(px_file)],
            capture_output=True,
            text=True,
            cwd=PROJECT_ROOT,
        )
        self.assertEqual(
            result.returncode, 0,
            msg=f"pythonjsx exited with {result.returncode}:\n{result.stderr}",
        )
        expected = txt_file.read_text()
        if result.stdout != expected:
            # Pretty-print the diff
            diff = difflib.unified_diff(expected.splitlines(), result.stdout.splitlines())
            self.fail(f"Output mismatch:\n{"\n".join(diff)}")

    test.__name__ = f"test_{px_file.stem}"
    return test


class TestRender(unittest.TestCase):
    pass


for _px in sorted(FIXTURES_DIR.glob("*.px")):
    _txt = _px.with_suffix(".txt")
    setattr(TestRender, f"test_{_px.stem}", _make_test(_px, _txt))
