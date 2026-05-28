"""Tests for the `.px` import hook.

`install()` adds a `MetaPathFinder` that looks for `foo.px` on `sys.path`
when you `import foo`.  These tests exercise that end-to-end by creating
a .px file in a temp directory, adding it to sys.path, and importing it.
"""

import os
import subprocess
import sys
import tempfile
import textwrap
import unittest

from pathlib import Path


class TestPxImporter(unittest.TestCase):
    PROJECT_ROOT = Path(__file__).parent.parent.parent

    def _run_import(self, px_sources: dict[str, str], runner: str) -> subprocess.CompletedProcess:
        """Write each `{module_name: .px source}` into a temp dir, register
        the import hook, and run `runner` (a string of Python code that
        imports the modules and prints something).  Returns the subprocess
        result so tests can assert on stdout/returncode/stderr."""
        with tempfile.TemporaryDirectory() as td:
            for name, src in px_sources.items():
                path = os.path.join(td, f"{name}.px")
                with open(path, "w", encoding="utf-8") as f:
                    f.write(src)
            harness = textwrap.dedent(f"""
                import sys
                import pythonjsx.importer
                pythonjsx.importer.install()
                sys.path.insert(0, {td!r})
            """) + runner
            return subprocess.run(
                [sys.executable, "-c", harness],
                cwd=self.PROJECT_ROOT,
                capture_output=True,
                text=True,
            )

    def test_importing_px_module_compiles_and_runs(self):
        result = self._run_import(
            {"hello": textwrap.dedent("""\
                def Greeter(name: str):
                    return <b>hi {name}</b>
            """)},
            runner="import hello; print(hello.Greeter(name='x').to_html())",
        )
        self.assertEqual(
            result.returncode, 0,
            msg=f"import failed:\nstdout={result.stdout!r}\nstderr={result.stderr!r}",
        )
        self.assertEqual(result.stdout.strip(), "<b>hi x</b>")

    def test_import_failure_surfaces_compile_error(self):
        # Mismatched tags → compiler exits non-zero → importer raises.
        result = self._run_import(
            {"broken": "x = <div></span>\n"},
            runner="import broken",
        )
        self.assertNotEqual(result.returncode, 0)
        # The stderr from the import attempt should contain something from
        # the compiler diagnostics.
        self.assertTrue(result.stderr.strip())

    def test_px_can_import_another_px(self):
        # One .px file imports another .px file as a module.  Both should
        # go through the hook.
        result = self._run_import(
            {
                "widget": textwrap.dedent("""\
                    def Widget():
                        return <span class="w">W</span>
                """),
                "app": textwrap.dedent("""\
                    from widget import Widget
                    RESULT = (<div><Widget/></div>).to_html()
                """),
            },
            runner="import app; print(app.RESULT)",
        )
        self.assertEqual(
            result.returncode, 0,
            msg=f"import failed:\nstdout={result.stdout!r}\nstderr={result.stderr!r}",
        )
        self.assertEqual(result.stdout.strip(), '<div><span class="w">W</span></div>')

    def test_install_is_idempotent(self):
        # Call install() twice — the finder should only appear once in
        # sys.meta_path and imports should still work.
        result = self._run_import(
            {"once": 'RESULT = (<p>once</p>).to_html()\n'},
            runner=textwrap.dedent("""
                import pythonjsx.importer
                pythonjsx.importer.install()  # second call
                import once
                print(once.RESULT)
            """),
        )
        self.assertEqual(
            result.returncode, 0,
            msg=f"subprocess failed: {result.stderr!r}",
        )
        self.assertEqual(result.stdout.strip(), "<p>once</p>")


if __name__ == "__main__":
    unittest.main()
