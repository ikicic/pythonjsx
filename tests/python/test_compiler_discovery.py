import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from pythonjsx._compiler_discovery import find_compiler, find_formatter, find_tool

_DISCOVERY_ENV_KEYS = (
    "PYTHONJSX_COMPILER",
    "PYTHONJSX_FORMATTER",
    "PYTHONJSX_PATH",
)


def _fake_binary(directory: Path, name: str) -> Path:
    path = directory / name
    path.write_text("")
    if os.name != "nt":
        path.chmod(0o755)
    return path


class TestCompilerDiscovery(unittest.TestCase):
    def setUp(self):
        self._saved_env = {key: os.environ.get(key) for key in _DISCOVERY_ENV_KEYS}
        for key in _DISCOVERY_ENV_KEYS:
            os.environ.pop(key, None)

    def tearDown(self):
        for key, value in self._saved_env.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value

    def test_executable_override_wins(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            override = _fake_binary(root, "pythonjsx")
            os.environ["PYTHONJSX_COMPILER"] = str(override)

            other = root / "other"
            other.mkdir()
            _fake_binary(other, "pythonjsx")
            os.environ["PYTHONJSX_PATH"] = str(other)

            self.assertEqual(find_compiler(), str(override.resolve()))

    def test_pythonjsx_path_is_searched_left_to_right(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            first = root / "first"
            second = root / "second"
            first.mkdir()
            second.mkdir()
            expected = _fake_binary(second, "pythonjsx")
            os.environ["PYTHONJSX_PATH"] = f"{first}{os.pathsep}{second}"

            self.assertEqual(find_compiler(), str(expected))

    def test_sys_executable_symlink_parent_not_resolve_target(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            system_bin = root / "system" / "bin"
            venv_bin = root / "venv" / "bin"
            system_bin.mkdir(parents=True)
            venv_bin.mkdir(parents=True)
            venv_tool = _fake_binary(venv_bin, "pythonjsx")
            _fake_binary(system_bin, "pythonjsx")

            system_python = system_bin / "python3"
            system_python.write_text("")
            venv_python = venv_bin / "python"
            venv_python.symlink_to(system_python)

            env = {**os.environ, "PATH": str(system_bin)}
            with mock.patch.object(sys, "executable", str(venv_python)):
                with mock.patch.dict(os.environ, env, clear=False):
                    self.assertEqual(find_compiler(), str(venv_tool))

    def test_sys_executable_parent_beats_path(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            venv_bin = root / "venv" / "bin"
            path_bin = root / "on_path"
            venv_bin.mkdir(parents=True)
            path_bin.mkdir()
            venv_tool = _fake_binary(venv_bin, "pythonjsx")
            _fake_binary(path_bin, "pythonjsx")

            fake_python = venv_bin / "python"
            fake_python.write_text("")

            env = {**os.environ, "PATH": str(path_bin)}
            with mock.patch.object(sys, "executable", str(fake_python)):
                with mock.patch.dict(os.environ, env, clear=False):
                    self.assertEqual(find_compiler(), str(venv_tool))

    def test_path_via_which(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            path_bin = root / "bin"
            path_bin.mkdir()
            tool = _fake_binary(path_bin, "pythonjsx")
            fake_python = root / "python"
            fake_python.write_text("")

            env = {**os.environ, "PATH": str(path_bin)}
            with mock.patch.object(sys, "executable", str(fake_python)):
                with mock.patch.dict(os.environ, env, clear=False):
                    self.assertEqual(find_compiler(), str(tool))

    def test_cargo_target_fallback(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            release = root / "target" / "release"
            release.mkdir(parents=True)
            tool = _fake_binary(release, "pythonjsx")
            fake_python = root / "python"
            fake_python.write_text("")

            env = {**os.environ, "PATH": ""}
            with mock.patch.object(sys, "executable", str(fake_python)):
                with mock.patch.dict(os.environ, env, clear=False):
                    with mock.patch("pythonjsx._compiler_discovery.Path.cwd", return_value=root):
                        self.assertEqual(find_compiler(), str(tool))

    def test_formatter_uses_formatter_override(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            override = _fake_binary(root, "pythonjsx-format")
            os.environ["PYTHONJSX_FORMATTER"] = str(override)

            self.assertEqual(find_formatter(), str(override.resolve()))

    def test_find_tool_returns_none_when_missing(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            fake_python = root / "python"
            fake_python.write_text("")
            env = {**os.environ, "PATH": ""}
            with mock.patch.object(sys, "executable", str(fake_python)):
                with mock.patch.dict(os.environ, env, clear=False):
                    with mock.patch(
                        "pythonjsx._compiler_discovery.Path.cwd",
                        return_value=root,
                    ):
                        self.assertIsNone(find_tool("pythonjsx-no-such-tool"))
