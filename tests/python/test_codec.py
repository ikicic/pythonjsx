"""Tests for the `# coding: pythonjsx` source codec.

Exercise both the direct `pythonjsx_decode` call (unit) and the
codec-through-the-tokenizer path (integration) by `exec`-ing a .py
file whose first line is a coding declaration.
"""

import os
import subprocess
import sys
import tempfile
import textwrap
import unittest

from pathlib import Path

from pythonjsx.codec import register, pythonjsx_decode


class TestPythonjsxDecode(unittest.TestCase):
    """Direct codec decode — the function Python calls when it reads a
    .py file with `# coding: pythonjsx`.  Input is bytes (what the file
    has), output is the compiled Python source string."""

    def test_decode_returns_compiled_python(self):
        src = b"x = <div>hello</div>\n"
        decoded, consumed = pythonjsx_decode(src)
        self.assertEqual(consumed, len(src))
        self.assertIn("_pjr_Tpl", decoded)
        self.assertIn("<div>hello</div>", decoded)

    def test_decode_accepts_memoryview(self):
        src = b"y = <span>ok</span>\n"
        mv = memoryview(src)
        decoded, _consumed = pythonjsx_decode(mv)
        self.assertIn("<span>ok</span>", decoded)

    def test_decode_raises_on_compile_error(self):
        # Mismatched tags → compiler errors out, codec translates to ValueError.
        src = b"bad = <div></span>\n"
        with self.assertRaises(ValueError) as cm:
            pythonjsx_decode(src)
        # Error message should surface something from the compiler diagnostics.
        self.assertTrue(str(cm.exception))


class TestCodecIntegration(unittest.TestCase):
    """End-to-end: a .py file with `# coding: pythonjsx` is imported by
    Python, its content decoded through our codec, and the resulting
    module behaves as if the user had written the compiled .py directly."""

    PROJECT_ROOT = Path(__file__).parent.parent.parent

    def _run_with_codec(self, py_source: str) -> subprocess.CompletedProcess:
        """Write `py_source` to a tempfile, exec it via a helper that
        registers the codec first, and return the process result."""
        with tempfile.TemporaryDirectory() as td:
            py_path = os.path.join(td, "probe.py")
            with open(py_path, "wb") as f:
                f.write(py_source.encode("utf-8"))
            # Helper harness: register the codec, import the tempfile,
            # print a result the test can assert on.
            runner = textwrap.dedent(f"""
                import sys
                import pythonjsx.codec
                pythonjsx.codec.register()
                sys.path.insert(0, {td!r})
                import probe
                print(probe.RESULT)
            """)
            return subprocess.run(
                [sys.executable, "-c", runner],
                cwd=self.PROJECT_ROOT,
                capture_output=True,
                text=True,
            )

    def test_coding_declaration_triggers_compile(self):
        source = textwrap.dedent("""\
            # coding: pythonjsx
            def Greeter(name: str):
                return <b>hi {name}</b>
            RESULT = Greeter(name="world").to_html()
        """)
        result = self._run_with_codec(source)
        self.assertEqual(
            result.returncode, 0,
            msg=f"subprocess failed:\nstdout={result.stdout!r}\nstderr={result.stderr!r}",
        )
        self.assertEqual(result.stdout.strip(), "<b>hi world</b>")

    def test_coding_declaration_on_second_line(self):
        # PEP 263: the encoding declaration may be on line 1 OR 2.
        source = textwrap.dedent("""\
            #!/usr/bin/env python3
            # coding: pythonjsx
            RESULT = (<p class="tag">ok</p>).to_html()
        """)
        result = self._run_with_codec(source)
        self.assertEqual(
            result.returncode, 0,
            msg=f"subprocess failed: {result.stderr!r}",
        )
        self.assertEqual(result.stdout.strip(), '<p class="tag">ok</p>')


class TestRegisterIsIdempotent(unittest.TestCase):
    """Registering the codec more than once mustn't raise or duplicate
    the CodecInfo — `register()` is the user-facing entry point and is
    likely to be called from multiple library init paths."""

    def test_multiple_register_calls_succeed(self):
        register()
        register()
        register()
        # If we got here, we're good.


if __name__ == "__main__":
    unittest.main()
