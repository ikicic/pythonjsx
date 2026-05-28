"""Django template-engine render benchmarks.

Renders the same scenarios (from `_bench_common.SCENARIOS`) as the PythonJSX
and Jinja2 entry points.  Uses a minimal Django settings configuration so
`django.template.Template` is available without a full project.

Django's built-in templates have no equivalent of Jinja2's `trim_blocks`
/ `lstrip_blocks`, so these source templates keep the `{% %}` tags on
their own lines and accept some extra whitespace in the rendered output.
It's a minor unfairness (Django renders a few more whitespace bytes than
the PX / Jinja2 outputs) but it mirrors the shape of what a Django user
actually writes.

Run directly:

    ./venv/bin/python benchmarks/bench_django.py
    ./venv/bin/python benchmarks/bench_django.py --filter listing
"""

from __future__ import annotations

import os
import sys
from typing import Any, Callable

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _bench_common as common  # noqa: E402

import django
from django.conf import settings as django_settings

if not django_settings.configured:
    django_settings.configure(
        TEMPLATES=[{"BACKEND": "django.template.backends.django.DjangoTemplates"}],
        USE_TZ=False,
    )
    django.setup()

from django.template import Template, Context  # noqa: E402


# ---------------------------------------------------------------------------
# Templates
# ---------------------------------------------------------------------------
# Multi-line triple-quoted so they line up visually with the PX templates in
# `bench_pythonjsx.py` and the Jinja2 templates in `bench_jinja2.py`.

DJ_CARD = '''\
<div class="grid">
{% for c in cards %}<div class="card"><h2>{{ c.title }}</h2><p>{{ c.body }}</p></div>{% endfor %}
</div>
'''

DJ_TABLE = '''\
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

DJ_ATTRS = '''\
<div>
{% for d in elems %}<span{% for k, v in d.attrs.items %} {{ k }}="{{ v }}"{% endfor %}>{{ d.text }}</span>{% endfor %}
</div>
'''

DJ_BLOG = '''\
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

DJ_PRODUCTS = '''\
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

DJ_LISTING = '''\
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

def _card_render(n: int) -> Callable[[], Any]:
    data = common.card_data(n)
    tpl = Template(DJ_CARD)
    ctx = Context({"cards": data})
    return lambda: tpl.render(ctx)


def _table_render(n: int, cols: int = 5) -> Callable[[], Any]:
    rows = common.table_data(n, cols)
    col_range = list(range(cols))
    tpl = Template(DJ_TABLE)
    ctx = Context({"rows": rows, "col_range": col_range})
    return lambda: tpl.render(ctx)


def _deep_render(depth: int) -> Callable[[], Any]:
    opens, closes = "", ""
    for i in range(depth):
        opens += f'<div class="d{i}">'
        closes = "</div>" + closes
    tpl = Template(opens + "<span>leaf</span>" + closes)
    ctx = Context({})
    return lambda: tpl.render(ctx)


def _attrs_render(n_elems: int, n_attrs: int = 20) -> Callable[[], Any]:
    data = common.attr_data(n_elems, n_attrs)
    tpl = Template(DJ_ATTRS)
    ctx = Context({"elems": data})
    return lambda: tpl.render(ctx)


def _blog_render(n_comments: int) -> Callable[[], Any]:
    data = common.blog_data(n_comments)
    tpl = Template(DJ_BLOG)
    ctx = Context({"post": data})
    return lambda: tpl.render(ctx)


def _products_render(n: int) -> Callable[[], Any]:
    data = common.products_data(n)
    tpl = Template(DJ_PRODUCTS)
    ctx = Context({"products": data})
    return lambda: tpl.render(ctx)


def _listing_render(n: int) -> Callable[[], Any]:
    data = common.listing_data(n)
    tpl = Template(DJ_LISTING)
    ctx = Context({"items": data})
    return lambda: tpl.render(ctx)


def main() -> None:
    args = common.create_parser(__doc__ or "").parse_args()
    print(f"Python {sys.version.split()[0]}, engine: Django")
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
        # i18n_listing: same story as Jinja2 — Django handles unicode uniformly.
        if name == "i18n_listing":
            return None
        return None

    common.run("Django", get_render_fn, args)


if __name__ == "__main__":
    main()
