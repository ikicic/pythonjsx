"""Shared data and runner helpers for per-engine benchmark scripts.

Each engine has its own benchmark entry point (`bench_pythonjsx.py`,
`bench_jinja2.py`, `bench_django.py`) so they can be run independently and so
engine-specific setup (like Django's `settings.configure()` or building the
PythonJSX `.px` files) doesn't leak into the other engines' timings.

What lives here:

- **Data factories** (`card_data`, `blog_data`, …).  Every engine renders the
  same dict/list data, so the cross-engine numbers measure template-engine
  work, not data-shape differences.
- **Shared fixture text** (`LOREM_PARAGRAPHS`, `LISTING_TITLES`,
  `LISTING_TITLES_HR`).  Lorem ipsum for the Latin-alphabet scenarios; a
  Croatian-flavoured variant with characteristic ć/č/š/ž/đ diacritics for
  the `i18n_listing` scenario, which forces the runtime's kind=2 write path
- **Scenario registry** (`SCENARIOS`) — a list of `(name, N)` pairs.  Each
  engine's runner consults this so every engine runs exactly the same suite.
- **Timing helper** and **CLI argument parsing**, so each entry point can
  be a thin shell.
"""

from __future__ import annotations

import argparse
import time
from typing import Any, Callable


# ---------------------------------------------------------------------------
# Fixture text
# ---------------------------------------------------------------------------

LOREM_TITLE = "Lorem ipsum dolor sit amet consectetur adipiscing elit"

# Eight paragraphs of classical Lorem ipsum at typical body-paragraph lengths.
LOREM_PARAGRAPHS = [
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor "
    "incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud "
    "exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.",
    "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat "
    "nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui "
    "officia deserunt mollit anim id est laborum.",
    "Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque "
    "laudantium, totam rem aperiam, eaque ipsa quae ab illo inventore veritatis et quasi "
    "architecto beatae vitae dicta sunt explicabo.",
    "Nemo enim ipsam voluptatem quia voluptas sit aspernatur aut odit aut fugit, sed quia "
    "consequuntur magni dolores eos qui ratione voluptatem sequi nesciunt. Neque porro "
    "quisquam est, qui dolorem ipsum quia dolor sit amet.",
    "Consectetur, adipisci velit, sed quia non numquam eius modi tempora incidunt ut labore "
    "et dolore magnam aliquam quaerat voluptatem. Ut enim ad minima veniam, quis nostrum "
    "exercitationem ullam corporis suscipit laboriosam.",
    "Nisi ut aliquid ex ea commodi consequatur. Quis autem vel eum iure reprehenderit qui in "
    "ea voluptate velit esse quam nihil molestiae consequatur, vel illum qui dolorem eum "
    "fugiat quo voluptas nulla pariatur.",
    "At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis praesentium "
    "voluptatum deleniti atque corrupti quos dolores et quas molestias excepturi sint "
    "occaecati cupiditate non provident.",
    "Similique sunt in culpa qui officia deserunt mollitia animi, id est laborum et dolorum "
    "fuga. Et harum quidem rerum facilis est et expedita distinctio. Nam libero tempore, "
    "cum soluta nobis est eligendi optio cumque nihil impedit.",
]

# Ten Latin-alphabet titles for the listing scenarios.  All ASCII so the
# runtime's kind=1 write path is exercised.
LISTING_TITLES = [
    "Lorem ipsum dolor sit amet consectetur",
    "Adipiscing elit sed do eiusmod tempor",
    "Incididunt ut labore et dolore magna",
    "Aliqua ut enim ad minim veniam quis",
    "Nostrud exercitation ullamco laboris",
    "Duis aute irure dolor in reprehenderit",
    "Voluptate velit esse cillum dolore eu",
    "Fugiat nulla pariatur excepteur sint",
    "Occaecat cupidatat non proident sunt",
    "Culpa qui officia deserunt mollit anim",
]

# Ten Croatian-flavoured titles laced with ć/č/š/ž/đ.  Any of those letters
# is above U+00FF so a Python str containing them is stored at kind=2
# (16-bit code units).  Forces the runtime's kind=2 write path even when
# surrounded by ASCII.
LISTING_TITLES_HR = [
    "Šuma čempresa uz obalu jadranskog mora",
    "Čarobni križ iznad staroga grada učitelja",
    "Različiti šišmiši lete iznad šumskih staza",
    "Đački dom čuva stare knjige o povijesti",
    "Žuta mačka šeta po snježnoj livadi zimi",
    "Ćošak sobe gdje držimo najvažnije papire",
    "Naš ljubičasti čovjek traži obiteljski đon",
    "Žuboreća rijeka teče kroz planinska naselja",
    "Šipražje kroz koje se probija hrabri putnik",
    "Današnji članak govori o ćelijama biljaka",
]


# ---------------------------------------------------------------------------
# Data factories
# ---------------------------------------------------------------------------

def card_data(n: int):
    return [{"title": f"Card {i}", "body": f"Body {i}"} for i in range(n)]


def table_data(n: int, cols: int = 5):
    return [tuple(f"r{r}c{c}" for c in range(cols)) for r in range(n)]


def attr_data(n_elems: int, n_attrs: int = 20):
    return [
        {"text": f"item{e}",
         "attrs": {f"data-a{a}": f"v{e}_{a}" for a in range(n_attrs)}}
        for e in range(n_elems)
    ]


def blog_data(n_comments: int):
    return {
        "title": LOREM_TITLE,
        "author": "Jane Doe",
        "author_id": 42,
        "date": "2026-04-19",
        "read_time": 7,
        "comment_count": n_comments,
        "tags": ["lorem", "ipsum", "dolor", "sit", "amet"],
        "paragraphs": LOREM_PARAGRAPHS,
        "comments": [
            {
                "id": i,
                "author": f"user_{i}",
                "date": "2026-04-19",
                "body": f"Lorem ipsum dolor sit amet, consectetur adipiscing elit. "
                        f"Comment number {i} with some placeholder text.",
            }
            for i in range(n_comments)
        ],
    }


def products_data(n: int):
    return [
        {
            "id": i,
            "sku": f"SKU-{i:06d}",
            "img": f"/static/products/{i}.jpg",
            "title": f"Lorem Product {i}",
            "desc": "Lorem ipsum dolor sit amet, consectetur adipiscing elit. "
                    "Sed do eiusmod tempor incididunt ut labore.",
            "price": f"{(i + 1) * 9.99:.2f}",
            "rating": (i % 5) + 1,
        }
        for i in range(n)
    ]


def listing_data(n: int):
    return [
        {
            "id": 10000 + i,
            "rank": i + 1,
            "title": LISTING_TITLES[i % len(LISTING_TITLES)],
            "url": f"https://example.com/item/{i}",
            "domain": f"example{i % 10}.com",
            "score": 50 + (i * 7) % 400,
            "author": f"user_{i}",
            "time_ago": f"{(i % 23) + 1} hours ago",
            "comments": (i * 3) % 250,
        }
        for i in range(n)
    ]


def i18n_listing_data(n: int):
    """Same shape as `listing_data` but with Croatian-diacritic-laced titles
    and Croatian-flavoured user-facing labels.  Exercises the kind=2
    write path."""
    return [
        {
            "id": 10000 + i,
            "rank": i + 1,
            "title": LISTING_TITLES_HR[i % len(LISTING_TITLES_HR)],
            "url": f"https://primjer.hr/članak/{i}",
            "domain": f"primjer{i % 10}.hr",
            "score": 50 + (i * 7) % 400,
            "author": f"korisnik_{i}",
            "time_ago": f"{(i % 23) + 1} sati prije",
            "comments": (i * 3) % 250,
        }
        for i in range(n)
    ]


# ---------------------------------------------------------------------------
# Scenario registry
# ---------------------------------------------------------------------------

# (name, N) pairs.  Each engine's runner looks up (name, N) in its own
# template map and invokes the corresponding render callable.  Scenarios
# whose name ends with `_i18n` or starts with `i18n_` should be treated by
# the runner as Latin-1-exceeding inputs (Croatian text) — important for
# engines that branch on str kind internally (i.e. PythonJSX).
SCENARIOS: list[tuple[str, int]] = [
    ("card",          10),
    ("card",         100),
    ("card",        1000),
    ("table",        100),
    ("table",       1000),
    ("table",       5000),
    ("deep",          20),
    ("deep",          50),
    ("deep",         150),
    ("attrs",         20),
    ("attrs",        100),
    ("attrs",        500),
    ("blog",          10),
    ("blog",          50),
    ("blog",         200),
    ("products",      24),
    ("products",     100),
    ("products",     500),
    ("listing",       30),
    ("listing",      100),
    ("listing",      500),
    ("i18n_listing",  30),
    ("i18n_listing", 100),
    ("i18n_listing", 500),
]


# ---------------------------------------------------------------------------
# Runner helpers
# ---------------------------------------------------------------------------

def time_fn(fn: Callable[[], Any], min_time_s: float = 0.5) -> float:
    """Return median per-call time in seconds.

    `perf_counter` itself has non-trivial overhead, so timing a single
    fast call is noisy.  We probe the per-call cost, then batch `N`
    calls per timed block so each block lasts ~1 ms — measurement
    overhead is divided by `N` and becomes negligible."""
    fn()  # warmup

    # Probe per-call cost to pick a batch size.
    t0 = time.perf_counter()
    fn()
    per_call_est = max(time.perf_counter() - t0, 1e-9)
    batch = max(1, int(0.001 / per_call_est))  # ~1 ms per block

    times: list[float] = []
    deadline = time.perf_counter() + min_time_s
    while True:
        t0 = time.perf_counter()
        for _ in range(batch):
            fn()
        dt = time.perf_counter() - t0
        times.append(dt / batch)
        if time.perf_counter() >= deadline and len(times) >= 3:
            break

    times.sort()
    return times[len(times) // 2]


def create_parser(description: str) -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=description)
    p.add_argument(
        "--filter", "-k",
        metavar="PATTERN",
        help="Run only scenarios whose label contains PATTERN (substring match).",
    )
    p.add_argument(
        "--duration",
        type=float,
        default=0.5,
        metavar="SECONDS",
        help="Minimum wall-clock time to spend on each scenario's timing "
             "loop (default: 0.5)."
    )
    return p


def run(
    engine_name: str,
    get_render_fn: Callable[[str, int], Callable[[], Any] | None],
    args: argparse.Namespace,
) -> None:
    """Drive the shared scenario list against one engine.

    `get_render_fn(name, n)` returns a zero-arg callable that renders the
    `(name, n)` scenario with this engine, or `None` if the engine does
    not implement this scenario (e.g. i18n scenarios that only PythonJSX
    bothers with).  Scenarios that return `None` are quietly skipped with
    a `—` row, which keeps table alignment across engines and makes
    missing cells visible."""
    scenarios = SCENARIOS
    if args.filter:
        scenarios = [s for s in scenarios if args.filter in f"{s[0]}_{s[1]}"]
        if not scenarios:
            print(f"No scenarios match --filter {args.filter!r}")
            return

    hdr = f"{'scenario':<16} {'N':>6}  {engine_name:>12}"
    print(hdr)
    print("-" * len(hdr))
    for name, n in scenarios:
        label = f"{name}_{n}"
        render_fn = get_render_fn(name, n)
        if render_fn is None:
            print(f"{label:<16} {n:>6}  {'—':>12}")
            continue
        seconds = time_fn(render_fn, min_time_s=args.duration)
        us = seconds * 1e6
        print(f"{label:<16} {n:>6}  {us:>10.2f}µs")
