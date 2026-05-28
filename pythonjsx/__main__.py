import os
import subprocess
import sys

from pythonjsx._compiler_discovery import (
    find_compiler as _find_compiler,
    find_formatter as _find_formatter,
)


def cmd_run(args: list[str]) -> None:
    if not args:
        print("Usage: python -m pythonjsx run <file.px> [args...]", file=sys.stderr)
        sys.exit(1)

    file_path = args[0]
    sys.argv = args  # script sees its own argv

    compiler = _find_compiler()
    if compiler is None:
        print("Error: pythonjsx compiler not found", file=sys.stderr)
        sys.exit(1)

    try:
        process = subprocess.Popen(
            [compiler, "compile", file_path],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        stdout, stderr = process.communicate()
        if process.returncode != 0:
            print(f"Compilation failed:\n{stderr.decode('utf-8', errors='replace')}", file=sys.stderr)
            sys.exit(1)
        compiled_code = stdout.decode('utf-8')
    except Exception as e:
        print(f"Error invoking compiler: {e}", file=sys.stderr)
        sys.exit(1)

    globals_dict = {
        "__name__": "__main__",
        "__file__": os.path.abspath(file_path),
        "__doc__": None,
        "__package__": None,
    }
    file_dir = os.path.dirname(os.path.abspath(file_path))
    if sys.path[0] != file_dir:
        sys.path.insert(0, file_dir)

    exec(compiled_code, globals_dict)


def cmd_compile(args: list[str]) -> None:
    if not args:
        print("Usage: python -m pythonjsx compile <file.px> [-o <out.py>]", file=sys.stderr)
        sys.exit(1)

    compiler = _find_compiler()
    if compiler is None:
        print("Error: pythonjsx compiler not found", file=sys.stderr)
        sys.exit(1)

    try:
        result = subprocess.run([compiler, "compile"] + args)
        sys.exit(result.returncode)
    except Exception as e:
        print(f"Error invoking compiler: {e}", file=sys.stderr)
        sys.exit(1)


def cmd_format(args: list[str]) -> None:
    if not args:
        print(
            "Usage: python -m pythonjsx format <file.px> [pythonjsx-format flags...]",
            file=sys.stderr,
        )
        sys.exit(1)

    formatter = _find_formatter()
    if formatter is None:
        print("Error: pythonjsx-format formatter not found", file=sys.stderr)
        sys.exit(1)

    try:
        result = subprocess.run([formatter] + args)
        sys.exit(result.returncode)
    except Exception as e:
        print(f"Error invoking formatter: {e}", file=sys.stderr)
        sys.exit(1)


def main() -> None:
    if len(sys.argv) < 2 or '--help' in sys.argv[1:3]:
        print("Usage: python -m pythonjsx <command> [args...]")
        print()
        print("Commands:")
        print("  run <file.px> [args...]                   Compile and run a .px file")
        print("  compile <file.px> [-o out.py]            Compile a .px file (stdout by default)")
        print("  format <file.px> [flags...]              Format JSX in a .px file")
        sys.exit(1)

    command = sys.argv[1]
    rest = sys.argv[2:]

    if command == "run":
        cmd_run(rest)
    elif command == "compile":
        cmd_compile(rest)
    elif command == "format":
        cmd_format(rest)
    else:
        print(f"Unknown command: {command!r}", file=sys.stderr)
        print("Commands: run, compile, format", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
