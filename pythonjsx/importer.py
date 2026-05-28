"""Import hook for `.px` files.

Register by calling `pythonjsx.importer.install()` at application startup.
After that, any `import foo` statement that finds `foo.px` on `sys.path`
triggers the compiler and the resulting Python code is exec()'d into the module
namespace.
"""

import importlib.abc
import importlib.machinery
import os
import subprocess
import sys

from pythonjsx._compiler_discovery import find_compiler

_compiler = None


def _find_compiler():
    global _compiler
    if _compiler is None:
        _compiler = find_compiler()
    return _compiler


# TODO: spawning a subprocess per .px module is costly — process startup
# dominates compile time for small files, and applications with many .px
# modules pay it N times on cold start. Two options:
#   (a) cache compiled output by source hash under __pycache__ so repeat
#       imports skip the subprocess entirely (gives us cold-start parity
#       with .py's .pyc caching).
#   (b) expose the Rust compiler as a native Python extension (pyo3), so
#       we never fork. This also replaces codec.py's subprocess.
def _compile_file(path):
    compiler = _find_compiler()
    if compiler is None:
        raise ImportError("pythonjsx compiler not found")

    result = subprocess.run(
        [compiler, "compile", path],
        capture_output=True,
    )
    if result.returncode != 0:
        raise ImportError(
            f"Failed to compile {path}: "
            f"{result.stderr.decode('utf-8', errors='replace')}")
    return result.stdout.decode("utf-8")


class PXFinder(importlib.abc.MetaPathFinder):
    def find_spec(self, fullname, path, target=None):
        module_name = fullname.split(".")[-1]
        search_paths = path if path else sys.path

        for dir_path in search_paths:
            if not isinstance(dir_path, str):
                continue
            px_file = os.path.join(dir_path, module_name + ".px")
            if os.path.isfile(px_file):
                return importlib.machinery.ModuleSpec(
                    fullname,
                    PXLoader(px_file),
                    origin=px_file,
                )
        return None


class PXLoader(importlib.abc.Loader):
    def __init__(self, path):
        self.path = path

    def create_module(self, spec):
        return None

    def exec_module(self, module):
        source = _compile_file(self.path)
        code = compile(source, self.path, "exec")
        exec(code, module.__dict__)
        module.__file__ = self.path
        if module.__spec__:
            module.__spec__._set_fileattr = True


def install():
    if not any(isinstance(f, PXFinder) for f in sys.meta_path):
        sys.meta_path.append(PXFinder())
