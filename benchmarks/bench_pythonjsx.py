"""PythonJSX render benchmarks.

Compiles the bundled `.px` templates once via the `pythonjsx` CLI,
imports them as Python modules, and times their `render(...).to_html()`
loops.  Same scenarios as `bench_jinja2.py` and `bench_django.py`, so the
outputs line up column-for-column across engines.

Run directly:

    ./venv/bin/python benchmarks/bench_pythonjsx.py
    ./venv/bin/python benchmarks/bench_pythonjsx.py --filter blog
    ./venv/bin/python benchmarks/bench_pythonjsx.py --duration 5     # longer warmup
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from types import ModuleType
from typing import Any, Callable

# Let this script be invoked as either `python benchmarks/bench_pythonjsx.py`
# or `python -m benchmarks.bench_pythonjsx`.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _bench_common as common  # noqa: E402


# ---------------------------------------------------------------------------
# Compiler helper
# ---------------------------------------------------------------------------

def _find_compiler() -> str:
    for p in ("./target/release/pythonjsx", "./target/debug/pythonjsx"):
        if os.path.isfile(p):
            return p
    raise RuntimeError("pythonjsx binary not found — run `cargo build`")


def _compile_and_load(
    name: str,
    px_source: str,
    compiler: str,
    tmpdir: str,
) -> ModuleType:
    """Write `.px` source to a temp file, compile it via the PythonJSX CLI,
    and load the resulting `.py` into a fresh module object.  Same path a
    user's code takes — no shortcuts."""
    px_path = os.path.join(tmpdir, f"{name}.px")
    py_path = os.path.join(tmpdir, f"{name}.py")
    with open(px_path, "w") as f:
        f.write(px_source)
    result = subprocess.run(
        [compiler, "compile", px_path, "-o", py_path],
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"Compilation of {name}.px failed:\n"
            f"{result.stderr.decode('utf-8', errors='replace')}"
        )
    mod = ModuleType(name)
    mod.__file__ = py_path
    with open(py_path) as f:
        code = compile(f.read(), py_path, "exec")
    exec(code, mod.__dict__)
    return mod


# ---------------------------------------------------------------------------
# .px templates
# ---------------------------------------------------------------------------

PX_CARD = '''\
def render(cards):
    return (
        <div class="grid">
            {<div class="card"><h2>{c["title"]}</h2><p>{c["body"]}</p></div> for c in cards}
        </div>
    )
'''

PX_TABLE = '''\
def render(rows, col_range):
    return (
        <table>
            <thead>
                <tr>
                    {<th>Col{c}</th> for c in col_range}
                </tr>
            </thead>
            <tbody>
                {<tr>{<td>{cell}</td> for cell in row}</tr> for row in rows}
            </tbody>
        </table>
    )
'''

PX_ATTRS = '''\
def render(elems):
    return (
        <div>
            {<span {**d["attrs"]}>{d["text"]}</span> for d in elems}
        </div>
    )
'''


def _make_px_deep(depth: int) -> str:
    """Generate a `.px` source with statically nested <div>s."""
    opens = "".join(f'<div class="d{i}">' for i in range(depth))
    closes = "</div>" * depth
    return f"def render():\n    return {opens}<span>leaf</span>{closes}\n"


# Nested components (Tag, Comment — both hoist-eligible), mixed content.
PX_BLOG = '''\
def Tag(name):
    return <a class="tag" href={f"/tag/{name}"}>{name}</a>

def Comment(c):
    return (
        <div class="comment" id={f"c-{c['id']}"}>
            <div class="meta">
                <strong>{c["author"]}</strong>
                <time>{c["date"]}</time>
            </div>
            <p class="body">{c["body"]}</p>
        </div>
    )

def render(post):
    return (
        <article class="post">
            <header>
                <h1>{post["title"]}</h1>
                <div class="meta">
                    by <a href={f"/user/{post['author_id']}"}>{post["author"]}</a>
                    <time>{post["date"]}</time>
                </div>
                <div class="tags">
                    {<Tag name={t}/> for t in post["tags"]}
                </div>
            </header>
            <div class="body">
                {<p>{para}</p> for para in post["paragraphs"]}
            </div>
            <section class="comments">
                <h2>{post["comment_count"]} Comments</h2>
                {<Comment c={c}/> for c in post["comments"]}
            </section>
        </article>
    )
'''

# Attribute-heavy: multiple static and dynamic attributes per item.
PX_PRODUCTS = '''\
def Card(p):
    return (
        <div class="card" data-id={p["id"]} data-sku={p["sku"]}>
            <img src={p["img"]} alt={p["title"]} loading="lazy"/>
            <h3 class="title">{p["title"]}</h3>
            <p class="desc">{p["desc"]}</p>
            <div class="price">${p["price"]}</div>
            <div class="rating" aria-label={f"{p['rating']} of 5 stars"}>
                <span class="stars">{p["rating"]}/5</span>
            </div>
            <button class="btn btn-add" data-action="add" data-id={p["id"]}>Add to cart</button>
        </div>
    )

def render(products):
    return (
        <div class="product-grid">
            {<Card p={p}/> for p in products}
        </div>
    )
'''

# Flat large-N listing with moderately rich per-item markup.
PX_LISTING = '''\
def Item(s):
    return (
        <tr class="item" id={f"item-{s['id']}"}>
            <td class="rank">{s["rank"]}.</td>
            <td class="votelinks">
                <a href={f"vote?id={s['id']}"}>up</a>
            </td>
            <td class="title">
                <a href={s["url"]} class="title-link">{s["title"]}</a>
                <span class="site">
                    (<a href={f"from?site={s['domain']}"}>{s["domain"]}</a>)
                </span>
                <div class="meta">
                    <span class="score">{s["score"]} points</span>
                    by <a href={f"user?id={s['author']}"}>{s["author"]}</a>
                    {s["time_ago"]}
                    | <a href={f"item?id={s['id']}"}>{s["comments"]} comments</a>
                </div>
            </td>
        </tr>
    )

def render(items):
    return (
        <table class="itemlist">
            <tbody>
                {<Item s={s}/> for s in items}
            </tbody>
        </table>
    )
'''

# Same shape as `PX_LISTING` but with Croatian-flavoured class names and
# labels.  Exercises the runtime's kind=2 write path because Croatian
# diacritics (ć, č, š, ž, đ) sit above U+00FF.
PX_I18N_LISTING = '''\
def Item(s):
    return (
        <tr class="članak" id={f"item-{s['id']}"}>
            <td class="broj">{s["rank"]}.</td>
            <td class="glasovi">
                <a href={f"vote?id={s['id']}"}>glas</a>
            </td>
            <td class="naslov">
                <a href={s["url"]} class="poveznica">{s["title"]}</a>
                <span class="stranica">
                    (<a href={f"from?site={s['domain']}"}>{s["domain"]}</a>)
                </span>
                <div class="opis">
                    <span class="bodovi">{s["score"]} bodova</span>
                    od <a href={f"user?id={s['author']}"}>{s["author"]}</a>
                    {s["time_ago"]}
                    | <a href={f"item?id={s['id']}"}>{s["comments"]} komentara</a>
                </div>
            </td>
        </tr>
    )

def render(items):
    return (
        <table class="popis">
            <tbody>
                {<Item s={s}/> for s in items}
            </tbody>
        </table>
    )
'''


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

def _print_compiled(label: str, mod: ModuleType) -> None:
    """Dump the compiled `.py` of a PX module with a banner."""
    sep = "=" * 76
    print(sep)
    print(f"# {label} — {mod.__file__}")
    print(sep)
    assert mod.__file__ is not None
    with open(mod.__file__) as f:
        print(f.read().rstrip())
    print()


def main() -> None:
    parser = common.create_parser(__doc__ or "")
    parser.add_argument(
        "--print-py",
        action="store_true",
        help="Print the compiled Python source of each PX template before "
             "running the benchmarks.",
    )
    args = parser.parse_args()
    compiler = _find_compiler()
    print(f"Python {sys.version.split()[0]}, compiler: {compiler}")
    print()

    tmpdir = tempfile.mkdtemp(prefix="pxbench_")
    try:
        # Compile once up front; subsequent renders hit the imported module.
        card_mod = _compile_and_load("bench_card", PX_CARD, compiler, tmpdir)
        table_mod = _compile_and_load("bench_table", PX_TABLE, compiler, tmpdir)
        attrs_mod = _compile_and_load("bench_attrs", PX_ATTRS, compiler, tmpdir)
        blog_mod = _compile_and_load("bench_blog", PX_BLOG, compiler, tmpdir)
        products_mod = _compile_and_load("bench_products", PX_PRODUCTS, compiler, tmpdir)
        listing_mod = _compile_and_load("bench_listing", PX_LISTING, compiler, tmpdir)
        i18n_listing_mod = _compile_and_load(
            "bench_i18n_listing", PX_I18N_LISTING, compiler, tmpdir)
        # Deep-nesting templates: static nesting baked into the .px source;
        # one module per depth.  CPython caps nested parens at ~200, so 150
        # is the realistic upper bound we benchmark at.
        deep_mods = {
            d: _compile_and_load(f"bench_deep_{d}", _make_px_deep(d), compiler, tmpdir)
            for d in (20, 50, 150)
        }

        if args.print_py:
            # Only print modules relevant to the current filter (empty =
            # everything).  Deep is keyed on depth; print each depth's
            # compiled output separately.
            def _matches(name: str, n: int | str = "") -> bool:
                label = f"{name}_{n}" if n != "" else name
                return not args.filter or args.filter in label

            if any(_matches("card", n) for n in (10, 100, 1000)):
                _print_compiled("card", card_mod)
            if any(_matches("table", n) for n in (100, 1000, 5000)):
                _print_compiled("table", table_mod)
            if any(_matches("attrs", n) for n in (20, 100, 500)):
                _print_compiled("attrs", attrs_mod)
            if any(_matches("blog", n) for n in (10, 50, 200)):
                _print_compiled("blog", blog_mod)
            if any(_matches("products", n) for n in (24, 100, 500)):
                _print_compiled("products", products_mod)
            if any(_matches("listing", n) for n in (30, 100, 500)):
                _print_compiled("listing", listing_mod)
            if any(_matches("i18n_listing", n) for n in (30, 100, 500)):
                _print_compiled("i18n_listing", i18n_listing_mod)
            for d in (20, 50, 150):
                if _matches("deep", d):
                    _print_compiled(f"deep_{d}", deep_mods[d])

        def get_render_fn(name: str, n: int) -> Callable[[], Any] | None:
            if name == "card":
                return lambda: card_mod.render(common.card_data(n)).to_html()
            if name == "table":
                return lambda: table_mod.render(common.table_data(n), range(5)).to_html()
            if name == "attrs":
                return lambda: attrs_mod.render(common.attr_data(n)).to_html()
            if name == "deep":
                return lambda: deep_mods[n].render().to_html()
            if name == "blog":
                return lambda: blog_mod.render(common.blog_data(n)).to_html()
            if name == "products":
                return lambda: products_mod.render(common.products_data(n)).to_html()
            if name == "listing":
                return lambda: listing_mod.render(common.listing_data(n)).to_html()
            if name == "i18n_listing":
                return lambda: i18n_listing_mod.render(common.i18n_listing_data(n)).to_html()
            return None

        common.run("PythonJSX", get_render_fn, args)
    finally:
        import shutil
        shutil.rmtree(tmpdir, ignore_errors=True)


if __name__ == "__main__":
    main()
