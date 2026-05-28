"""Benchmark PythonJSX compilation speed (.px → .py).

Measures how fast the compiler turns .px source into .py output, across
various template sizes. The main goal is verifying O(N) scaling — the
absolute numbers are less important than the slope.
"""

from __future__ import annotations

import os
import subprocess
import tempfile
import time

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _find_compiler() -> str:
    for p in ("./target/release/pythonjsx", "./target/debug/pythonjsx"):
        if os.path.isfile(p):
            return p
    raise RuntimeError("pythonjsx binary not found — run `cargo build --release`")


def _generate_px_source(kind: str, n: int) -> str:
    """Return a .px source string of the given kind and size parameter."""
    if kind == "table":
        cols = 5
        rows = []
        for r in range(n):
            cells = "".join(f"<td>r{r}c{c}</td>" for c in range(cols))
            rows.append(f"        <tr>{cells}</tr>")
        header = "<tr>" + "".join(f"<th>Col {c}</th>" for c in range(cols)) + "</tr>"
        body = "\n".join(rows)
        return (
            f"def render():\n"
            f"    return (\n"
            f'        <table>\n'
            f"            <thead>{header}</thead>\n"
            f"            <tbody>\n"
            f"{body}\n"
            f"            </tbody>\n"
            f"        </table>\n"
            f"    )\n"
        )
    elif kind == "deep":
        inner = '"leaf"'
        for i in range(n):
            inner = f"<div class=\"d{i}\">{{{inner}}}</div>"
        return f"def render():\n    return {inner}\n"
    elif kind == "cards":
        cards = []
        for i in range(n):
            cards.append(
                f'        <div class="card"><h2>Card {i}</h2><p>Body {i}</p></div>'
            )
        body = "\n".join(cards)
        return (
            f"def render():\n"
            f"    return (\n"
            f'        <div class="grid">\n'
            f"{body}\n"
            f"        </div>\n"
            f"    )\n"
        )
    elif kind == "attrs":
        elems = []
        n_attrs = 20
        for e in range(n):
            attrs = " ".join(f'data-a{a}="v{e}_{a}"' for a in range(n_attrs))
            elems.append(f"        <span {attrs}>item{e}</span>")
        body = "\n".join(elems)
        return (
            f"def render():\n"
            f"    return (\n"
            f"        <div>\n"
            f"{body}\n"
            f"        </div>\n"
            f"    )\n"
        )
    else:
        raise ValueError(f"unknown kind: {kind}")


def bench_compile_one(compiler: str, px_source: str, iterations: int = 5) -> dict:
    """Time the compiler on `px_source`. Returns {median_ms, source_bytes}."""
    with tempfile.NamedTemporaryFile(suffix=".px", mode="w", delete=False) as f:
        f.write(px_source)
        f.flush()
        path = f.name

    try:
        # Warm up (ensure binary is in page cache).
        subprocess.run([compiler, "compile", path], capture_output=True, check=False)

        times = []
        for _ in range(iterations):
            t0 = time.perf_counter()
            subprocess.run([compiler, "compile", path], capture_output=True, check=False)
            t1 = time.perf_counter()
            times.append((t1 - t0) * 1000)

        times.sort()
        median = times[len(times) // 2]
        return {"median_ms": median, "source_bytes": len(px_source.encode())}
    finally:
        os.unlink(path)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

SCENARIOS = [
    ("cards",  [10, 50, 100, 500, 1000, 2000]),
    ("table",  [10, 50, 100, 500, 1000, 5000]),
    ("deep",   [10, 50, 100, 200, 500]),
    ("attrs",  [10, 50, 100, 500]),
]


def main():
    compiler = _find_compiler()
    print(f"Compiler: {compiler}")
    print(f"{'scenario':<20} {'N':>6} {'bytes':>8} {'median ms':>10} {'ms/KB':>10}")
    print("-" * 60)
    for kind, sizes in SCENARIOS:
        for n in sizes:
            src = _generate_px_source(kind, n)
            result = bench_compile_one(compiler, src)
            kb = result["source_bytes"] / 1024
            ms_per_kb = result["median_ms"] / kb if kb > 0 else 0
            print(
                f"{kind + '_' + str(n):<20} {n:>6} "
                f"{result['source_bytes']:>8} "
                f"{result['median_ms']:>9.2f}ms "
                f"{ms_per_kb:>9.2f}"
            )
    print()


if __name__ == "__main__":
    main()
