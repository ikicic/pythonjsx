"""Locate PythonJSX Rust tool binaries.

This is the single source of truth used by `importer.py`, `codec.py`, and
`__main__.py`.  It returns the first match among:

  1. The requested tool on `$PATH`.
  2. `./target/release/<tool>` (relative to the current working
     directory — dev builds from `cargo build --release`).
  3. `./target/debug/<tool>` (fallback for dev builds from `cargo
     build`).

Returns `None` if nothing is found; callers decide how to report the
failure (ImportError, CLI exit, ValueError from a codec, …).
"""

from __future__ import annotations

import shutil
from pathlib import Path


def find_tool(name: str) -> str | None:
    """Return the path/name of a PythonJSX Rust tool, or None if not found."""
    on_path = shutil.which(name)
    if on_path:
        return on_path
    cwd_target = Path.cwd() / "target"
    for subdir in ("release", "debug"):
        candidate = cwd_target / subdir / name
        if candidate.exists():
            return str(candidate)
    return None


def find_compiler() -> str | None:
    """Return the path/name of the pythonjsx compiler, or None if not found."""
    return find_tool("pythonjsx")


def find_formatter() -> str | None:
    """Return the path/name of the pythonjsx-format tool, or None if not found."""
    return find_tool("pythonjsx-format")
