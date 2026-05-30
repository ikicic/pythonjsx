"""Locate PythonJSX Rust tool binaries.

This is the single source of truth used by `importer.py`, `codec.py`, and
`__main__.py`.  For each tool, the first match wins:

  1. Per-tool executable override (`PYTHONJSX_COMPILER` or
     `PYTHONJSX_FORMATTER`) — full path to the binary.
  2. Directories in `PYTHONJSX_PATH` (`os.pathsep`-separated), left to right.
  3. The directory containing `sys.executable` (e.g. `venv/bin/`).
  4. `$PATH` via `shutil.which`.
  5. `./target/release/<tool>`, then `./target/debug/<tool>` (relative to the
     current working directory — dev builds from `cargo build`).

Returns `None` if nothing is found; callers decide how to report the
failure (ImportError, CLI exit, ValueError from a codec, …).
"""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path

_TOOL_OVERRIDES: dict[str, str] = {
    "pythonjsx": "PYTHONJSX_COMPILER",
    "pythonjsx-format": "PYTHONJSX_FORMATTER",
}


def _executable_in_directory(directory: Path, name: str) -> Path | None:
    if not directory.is_dir():
        return None
    candidates = [directory / name]
    if os.name == "nt" and not name.lower().endswith(".exe"):
        candidates.append(directory / f"{name}.exe")
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


def _search_directories(name: str, directories: list[Path]) -> str | None:
    for directory in directories:
        found = _executable_in_directory(directory, name)
        if found is not None:
            return str(found)
    return None


def _pythonjsx_path_directories() -> list[Path]:
    raw = os.environ.get("PYTHONJSX_PATH")
    if not raw:
        return []
    return [Path(part) for part in raw.split(os.pathsep) if part]


def _cargo_target_directories() -> list[Path]:
    cwd_target = Path.cwd() / "target"
    return [cwd_target / subdir for subdir in ("release", "debug")]


def find_tool(name: str) -> str | None:
    """Return the path/name of a PythonJSX Rust tool, or None if not found."""
    env_var = _TOOL_OVERRIDES.get(name)
    if env_var is not None:
        override = os.environ.get(env_var)
        if override:
            path = Path(override)
            if path.is_file():
                return str(path.resolve())

    found = _search_directories(name, _pythonjsx_path_directories())
    if found is not None:
        return found

    found = _executable_in_directory(Path(sys.executable).absolute().parent, name)
    if found is not None:
        return str(found)

    on_path = shutil.which(name)
    if on_path:
        return on_path

    found = _search_directories(name, _cargo_target_directories())
    if found is not None:
        return found

    return None


def find_compiler() -> str | None:
    """Return the path/name of the pythonjsx compiler, or None if not found."""
    return find_tool("pythonjsx")


def find_formatter() -> str | None:
    """Return the path/name of the pythonjsx-format tool, or None if not found."""
    return find_tool("pythonjsx-format")
