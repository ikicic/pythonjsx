import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


PROJECT_ROOT = Path(__file__).parent.parent.parent


class TestPythonjsxFormatCommand(unittest.TestCase):
    def run_pythonjsx_format(self, args):
        env = dict(os.environ)
        target_debug = PROJECT_ROOT / "target" / "debug"
        env["PATH"] = f"{target_debug}{os.pathsep}{env.get('PATH', '')}"
        return subprocess.run(
            [sys.executable, "-m", "pythonjsx", "format", *args],
            capture_output=True,
            text=True,
            cwd=PROJECT_ROOT,
            env=env,
        )

    def test_format_writes_stdout_by_default(self):
        with tempfile.TemporaryDirectory() as td:
            px_file = Path(td) / "input.px"
            px_file.write_text("x = <a\n    href={url}\n>\n    Home\n</a>\n")

            result = self.run_pythonjsx_format([str(px_file)])

            self.assertEqual(result.returncode, 0, msg=result.stderr)
            self.assertEqual(
                result.stdout,
                "x = (\n    <a href={url}>\n        Home\n    </a>\n)\n",
            )
            self.assertEqual(px_file.read_text(), "x = <a\n    href={url}\n>\n    Home\n</a>\n")

    def test_format_in_place_passes_flags_through(self):
        with tempfile.TemporaryDirectory() as td:
            px_file = Path(td) / "input.px"
            px_file.write_text("x = <a\n    href={url}\n>\n    Home\n</a>\n")

            result = self.run_pythonjsx_format([str(px_file), "-i"])

            self.assertEqual(result.returncode, 0, msg=result.stderr)
            self.assertEqual(result.stdout, "")
            self.assertEqual(
                px_file.read_text(),
                "x = (\n    <a href={url}>\n        Home\n    </a>\n)\n",
            )

    def test_format_passes_collapse_multiline_flag_through(self):
        with tempfile.TemporaryDirectory() as td:
            px_file = Path(td) / "input.px"
            px_file.write_text("x = <a\n    href={url}\n>\n    Home\n</a>\n")

            result = self.run_pythonjsx_format([str(px_file), "--collapse-multiline"])

            self.assertEqual(result.returncode, 0, msg=result.stderr)
            self.assertEqual(result.stdout, "x = <a href={url}>Home</a>\n")


if __name__ == "__main__":
    unittest.main()
