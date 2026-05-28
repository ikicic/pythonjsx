"""Jinja2 render benchmarks.

Renders the same scenarios (from `_bench_common.SCENARIOS`) as the PythonJSX
and Django entry points, so the printed columns line up when compared
across engines.  `autoescape=True` is on throughout — the other engines
escape by default, and comparing against an unescaped Jinja2 setup would
measure it doing strictly less work.  `trim_blocks` and `lstrip_blocks`
are on so multi-line templates don't produce extra whitespace that the
other engines' templates don't.

Run directly:

    ./venv/bin/python benchmarks/bench_jinja2.py
    ./venv/bin/python benchmarks/bench_jinja2.py --filter products
"""

from __future__ import annotations

import os
import sys
from typing import Any, Callable

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _bench_common as common  # noqa: E402

from jinja2 import Environment

_env = Environment(autoescape=True, trim_blocks=True, lstrip_blocks=True)


# ---------------------------------------------------------------------------
# Templates
# ---------------------------------------------------------------------------
# Multi-line triple-quoted so they line up visually with the PX templates in
# `bench_pythonjsx.py`.  `{% for … %}` / `{% endfor %}` are on lines by
# themselves; `trim_blocks` + `lstrip_blocks` strip the surrounding
# whitespace so the rendered output is compact.

J2_CARD = '''\
<div class="grid">
{% for c in cards %}<div class="card"><h2>{{ c.title }}</h2><p>{{ c.body }}</p></div>{% endfor %}
</div>
'''

J2_TABLE = '''\
<table>
    <thead>
        <tr>
{% for c in col_range %}<th>Col{{ c }}</th>{% endfor %}
        </tr>
    </thead>
    <tbody>
{% for row in rows %}<tr>{% for cell in row %}<td>{{ cell }}</td>{% endfor %}</tr>{% endfor %}
    </tbody>
</table>
'''

J2_ATTRS = '''\
<div>
{% for d in elems %}<span{{ d.attrs|xmlattr }}>{{ d.text }}</span>{% endfor %}
</div>
'''

J2_BLOG = '''\
<article class="post">
    <header>
        <h1>{{ post.title }}</h1>
        <div class="meta">
            by <a href="/user/{{ post.author_id }}">{{ post.author }}</a>
            <time>{{ post.date }}</time>
        </div>
        <div class="tags">
{% for t in post.tags %}<a class="tag" href="/tag/{{ t }}">{{ t }}</a>{% endfor %}
        </div>
    </header>
    <div class="body">
{% for para in post.paragraphs %}<p>{{ para }}</p>{% endfor %}
    </div>
    <section class="comments">
        <h2>{{ post.comment_count }} Comments</h2>
{% for c in post.comments %}
<div class="comment" id="c-{{ c.id }}">
    <div class="meta"><strong>{{ c.author }}</strong><time>{{ c.date }}</time></div>
    <p class="body">{{ c.body }}</p>
</div>
{% endfor %}
    </section>
</article>
'''

J2_PRODUCTS = '''\
<div class="product-grid">
{% for p in products %}
<div class="card" data-id="{{ p.id }}" data-sku="{{ p.sku }}">
    <img src="{{ p.img }}" alt="{{ p.title }}" loading="lazy"/>
    <h3 class="title">{{ p.title }}</h3>
    <p class="desc">{{ p.desc }}</p>
    <div class="price">${{ p.price }}</div>
    <div class="rating" aria-label="{{ p.rating }} of 5 stars">
        <span class="stars">{{ p.rating }}/5</span>
    </div>
    <button class="btn btn-add" data-action="add" data-id="{{ p.id }}">Add to cart</button>
</div>
{% endfor %}
</div>
'''

J2_LISTING = '''\
<table class="itemlist">
    <tbody>
{% for s in items %}
<tr class="item" id="item-{{ s.id }}">
    <td class="rank">{{ s.rank }}.</td>
    <td class="votelinks"><a href="vote?id={{ s.id }}">up</a></td>
    <td class="title">
        <a href="{{ s.url }}" class="title-link">{{ s.title }}</a>
        <span class="site">(<a href="from?site={{ s.domain }}">{{ s.domain }}</a>)</span>
        <div class="meta">
            <span class="score">{{ s.score }} points</span>
            by <a href="user?id={{ s.author }}">{{ s.author }}</a>
            {{ s.time_ago }}
            | <a href="item?id={{ s.id }}">{{ s.comments }} comments</a>
        </div>
    </td>
</tr>
{% endfor %}
    </tbody>
</table>
'''


# ---------------------------------------------------------------------------
# Render-factory helpers
# ---------------------------------------------------------------------------
# Compile once per scenario (same amortisation pattern the other engines use),
# then return a zero-arg render closure.

def _card_render(n: int) -> Callable[[], Any]:
    data = common.card_data(n)
    tpl = _env.from_string(J2_CARD)
    return lambda: tpl.render(cards=data)


def _table_render(n: int, cols: int = 5) -> Callable[[], Any]:
    rows = common.table_data(n, cols)
    col_range = list(range(cols))
    tpl = _env.from_string(J2_TABLE)
    return lambda: tpl.render(rows=rows, col_range=col_range)


def _deep_render(depth: int) -> Callable[[], Any]:
    # Each depth needs a freshly-built source string; compile once per depth.
    opens, closes = "", ""
    for i in range(depth):
        opens += f'<div class="d{i}">'
        closes = "</div>" + closes
    tpl = _env.from_string(opens + "<span>leaf</span>" + closes)
    return lambda: tpl.render()


def _attrs_render(n_elems: int, n_attrs: int = 20) -> Callable[[], Any]:
    # `xmlattr` is Jinja2's built-in idiom for "spread this dict as HTML
    # attributes" — handles None/False-omit + True-bare and is marked-safe
    # so autoescape doesn't re-escape it.  Matches the `{**d["attrs"]}`
    # PythonJSX form idiom-for-idiom.
    data = common.attr_data(n_elems, n_attrs)
    tpl = _env.from_string(J2_ATTRS)
    return lambda: tpl.render(elems=data)


def _blog_render(n_comments: int) -> Callable[[], Any]:
    data = common.blog_data(n_comments)
    tpl = _env.from_string(J2_BLOG)
    return lambda: tpl.render(post=data)


def _products_render(n: int) -> Callable[[], Any]:
    data = common.products_data(n)
    tpl = _env.from_string(J2_PRODUCTS)
    return lambda: tpl.render(products=data)


def _listing_render(n: int) -> Callable[[], Any]:
    data = common.listing_data(n)
    tpl = _env.from_string(J2_LISTING)
    return lambda: tpl.render(items=data)


def main() -> None:
    args = common.create_parser(__doc__ or "").parse_args()
    print(f"Python {sys.version.split()[0]}, engine: Jinja2")
    print()

    def get_render_fn(name: str, n: int) -> Callable[[], Any] | None:
        if name == "card":
            return _card_render(n)
        if name == "table":
            return _table_render(n)
        if name == "attrs":
            return _attrs_render(n)
        if name == "deep":
            return _deep_render(n)
        if name == "blog":
            return _blog_render(n)
        if name == "products":
            return _products_render(n)
        if name == "listing":
            return _listing_render(n)
        # `i18n_listing` — Jinja2 handles unicode uniformly so a Croatian
        # listing is measured by the `listing` scenario equally well.
        # Return None to leave the cell blank and keep rows aligned with
        # PythonJSX, where the kind=2 write path matters.
        if name == "i18n_listing":
            return None
        return None

    common.run("Jinja2", get_render_fn, args)


if __name__ == "__main__":
    main()
