"""Template-protocol runtime tests.

Exercises `JSXTemplate` / `JSXResult` / `SLOT_VALUE` / `SLOT_SPREAD` /
`SlotAttr`.  Pins the semantics ahead of the compiler's switch to
emitting the template protocol.
"""

import unittest

from pythonjsx.runtime import (
    JSXTemplate,
    SafeStr,
    SLOT_SPREAD,
    SLOT_VALUE,
    SlotAttr,
    PythonJSXValueError
)


class TestTemplateBasics(unittest.TestCase):
    def test_pure_static(self):
        tpl = JSXTemplate("<div>Hello</div>")
        self.assertEqual(tpl().to_html(), "<div>Hello</div>")
        # Adjacent literals fuse at construction; pure-static has 1 instr.
        self.assertEqual(tpl.n_instrs, 1)
        self.assertEqual(tpl.n_slots, 0)

    def test_empty_template(self):
        tpl = JSXTemplate()
        self.assertEqual(tpl().to_html(), "")
        self.assertEqual(tpl.n_instrs, 0)

    def test_adjacent_literals_fused(self):
        tpl = JSXTemplate("<div>", "Hello, ", "World!", "</div>")
        self.assertEqual(tpl.n_instrs, 1)
        self.assertEqual(tpl().to_html(), "<div>Hello, World!</div>")

    def test_empty_string_literals_dropped(self):
        tpl = JSXTemplate("", "<div>", "", "</div>", "")
        self.assertEqual(tpl.n_instrs, 1)
        self.assertEqual(tpl().to_html(), "<div></div>")

    def test_one_slot_text(self):
        tpl = JSXTemplate("<div>Hello, ", SLOT_VALUE, "!</div>")
        self.assertEqual(tpl("World").to_html(), "<div>Hello, World!</div>")

    def test_multi_slot(self):
        tpl = JSXTemplate("<p>A: ", SLOT_VALUE, ", B: ", SLOT_VALUE, "</p>")
        self.assertEqual(tpl(1, 2).to_html(), "<p>A: 1, B: 2</p>")

    def test_arg_count_mismatch(self):
        tpl = JSXTemplate("<a>", SLOT_VALUE, "</a>")
        with self.assertRaises(TypeError):
            tpl()
        with self.assertRaises(TypeError):
            tpl("x", "y")

    def test_rejects_unknown_chunk_type(self):
        with self.assertRaises(TypeError):
            JSXTemplate("<a>", 42, "</a>")


class TestSlotValueEscaping(unittest.TestCase):
    def setUp(self):
        self.tpl = JSXTemplate("<div>", SLOT_VALUE, "</div>")

    def test_str_escape(self):
        self.assertEqual(
            self.tpl("<script>alert(&)</script>").to_html(),
            "<div>&lt;script&gt;alert(&amp;)&lt;/script&gt;</div>",
        )

    def test_no_escape_needed_str(self):
        self.assertEqual(self.tpl("plain").to_html(), "<div>plain</div>")

    def test_int_no_escape(self):
        self.assertEqual(self.tpl(42).to_html(), "<div>42</div>")

    def test_float_no_escape(self):
        self.assertEqual(self.tpl(3.14).to_html(), "<div>3.14</div>")

    def test_none_omitted(self):
        self.assertEqual(self.tpl(None).to_html(), "<div></div>")

    def test_bool_omitted(self):
        # Matches the opcode-protocol behavior: bools render as nothing in
        # content position; only in attr position do they take on the
        # bare-attribute / omit-attribute semantics.
        self.assertEqual(self.tpl(True).to_html(), "<div></div>")
        self.assertEqual(self.tpl(False).to_html(), "<div></div>")

    def test_list(self):
        self.assertEqual(
            self.tpl(["a", "b", "<c>"]).to_html(),
            "<div>ab&lt;c&gt;</div>",
        )

    def test_tuple(self):
        self.assertEqual(self.tpl(("x", "y")).to_html(), "<div>xy</div>")

    def test_generator(self):
        self.assertEqual(
            self.tpl(str(i) for i in range(3)).to_html(),
            "<div>012</div>",
        )


class TestSlotAttr(unittest.TestCase):
    def setUp(self):
        self.tpl = JSXTemplate("<div", SlotAttr("id"), ">x</div>")

    def test_str_value(self):
        self.assertEqual(self.tpl("main").to_html(), '<div id="main">x</div>')

    def test_escape(self):
        self.assertEqual(
            self.tpl('a"b&c').to_html(),
            '<div id="a&quot;b&amp;c">x</div>',
        )

    def test_none_omits_attr(self):
        self.assertEqual(self.tpl(None).to_html(), "<div>x</div>")

    def test_false_omits_attr(self):
        self.assertEqual(self.tpl(False).to_html(), "<div>x</div>")

    def test_true_bare_attr(self):
        tpl = JSXTemplate("<input", SlotAttr("disabled"))
        self.assertEqual(tpl(True).to_html(), "<input disabled")

    def test_int_value_stringified(self):
        self.assertEqual(self.tpl(42).to_html(), '<div id="42">x</div>')


class TestSlotSpread(unittest.TestCase):
    def test_dict_spread(self):
        tpl = JSXTemplate("<div", SLOT_SPREAD, ">")
        self.assertEqual(
            tpl({"id": "x", "class": "y"}).to_html(),
            '<div id="x" class="y">',
        )

    def test_spread_value_escape(self):
        tpl = JSXTemplate("<div", SLOT_SPREAD, ">")
        self.assertEqual(
            tpl({"title": '<">'}).to_html(),
            '<div title="&lt;&quot;&gt;">',
        )

    def test_spread_key_rejects_invalid_chars(self):
        # Spread keys are arbitrary runtime data.  There's no safe way
        # to escape a forbidden char into an HTML attribute name (entity
        # refs aren't decoded inside names, a space silently injects a
        # second attribute), so the runtime rejects them outright.
        tpl = JSXTemplate("<div", SLOT_SPREAD, ">")
        for bad in ["><script>", "my key", 'a"b', "a=b", ""]:
            with self.assertRaises(PythonJSXValueError):
                tpl({bad: "x"}).to_html()

    def test_spread_none_omits(self):
        tpl = JSXTemplate("<div", SLOT_SPREAD, ">")
        self.assertEqual(
            tpl({"id": "x", "hidden": None}).to_html(),
            '<div id="x">',
        )

    def test_spread_bool_semantics(self):
        tpl = JSXTemplate("<div", SLOT_SPREAD, ">")
        self.assertEqual(
            tpl({"disabled": True, "readonly": False}).to_html(),
            "<div disabled>",
        )

    def test_spread_non_dict_mapping(self):
        class M:
            def keys(self):
                return ["a", "b"]

            def __getitem__(self, k):
                return {"a": "1", "b": "2"}[k]

        tpl = JSXTemplate("<div", SLOT_SPREAD, ">")
        self.assertEqual(tpl(M()).to_html(), '<div a="1" b="2">')


class TestComposition(unittest.TestCase):
    def test_jsxresult_as_slot_value(self):
        inner = JSXTemplate("<span>", SLOT_VALUE, "</span>")
        outer = JSXTemplate("<div>", SLOT_VALUE, "</div>")
        self.assertEqual(
            outer(inner("hello")).to_html(),
            "<div><span>hello</span></div>",
        )

    def test_nested_two_deep(self):
        leaf = JSXTemplate("<i>", SLOT_VALUE, "</i>")
        mid = JSXTemplate("<b>", SLOT_VALUE, "</b>")
        top = JSXTemplate("<p>", SLOT_VALUE, "</p>")
        self.assertEqual(
            top(mid(leaf("x"))).to_html(),
            "<p><b><i>x</i></b></p>",
        )

    def test_iterable_of_jsxresults(self):
        ul = JSXTemplate("<ul>", SLOT_VALUE, "</ul>")
        li = JSXTemplate("<li>", SLOT_VALUE, "</li>")
        # Generator — the common shape for a `{<X/> for x in …}` JSX loop.
        self.assertEqual(
            ul(li(x) for x in ["a", "b", "c"]).to_html(),
            "<ul><li>a</li><li>b</li><li>c</li></ul>",
        )

class TestNonAscii(unittest.TestCase):
    def test_kind_2_literal(self):
        # Croatian diacritics (ć, č, š, ž, đ) are above U+00FF, so the
        # containing str is kind=2.  Exercises the wider-kind write path.
        tpl = JSXTemplate('<div class="članak">', SLOT_VALUE, "</div>")
        self.assertEqual(
            tpl("hello").to_html(),
            '<div class="članak">hello</div>',
        )

    def test_kind_2_user_value(self):
        tpl = JSXTemplate("<span>", SLOT_VALUE, "</span>")
        self.assertEqual(tpl("Привет").to_html(), "<span>Привет</span>")

    def test_kind_4_user_value(self):
        # Astral-plane char forces kind=4.
        tpl = JSXTemplate("<span>", SLOT_VALUE, "</span>")
        self.assertEqual(tpl("Hi 😀").to_html(), "<span>Hi 😀</span>")

    def test_mixed_kinds_across_slots(self):
        tpl = JSXTemplate("<p>", SLOT_VALUE, " / ", SLOT_VALUE, "</p>")
        self.assertEqual(
            tpl("plain", "Привет").to_html(),
            "<p>plain / Привет</p>",
        )


class TestIntrospection(unittest.TestCase):
    def test_properties(self):
        tpl = JSXTemplate("<a", SlotAttr("href"), ">", SLOT_VALUE, "</a>")
        self.assertEqual(tpl.n_slots, 2)
        # Five instructions: "<a", href attr, ">", content slot, "</a>"
        # — no fusion possible since slots separate the literals.
        self.assertEqual(tpl.n_instrs, 5)

    def test_slot_attr_stores_name(self):
        # SlotAttr stores the raw compile-time-known attr name; the
        # grammar restricts these to identifier-like tokens so no
        # name-side escaping is needed at render time.
        sa = SlotAttr("data-id")
        self.assertEqual(sa.name, "data-id")


class TestResultReuse(unittest.TestCase):
    def test_to_html_idempotent(self):
        tpl = JSXTemplate("<div>", SLOT_VALUE, "</div>")
        result = tpl("hello")
        self.assertEqual(result.to_html(), "<div>hello</div>")
        self.assertEqual(result.to_html(), "<div>hello</div>")

    def test_str_equals_to_html(self):
        tpl = JSXTemplate("<div>", SLOT_VALUE, "</div>")
        result = tpl("x")
        self.assertEqual(str(result), result.to_html())


class TestSafeStr(unittest.TestCase):
    def test_is_not_str(self):
        # Deliberately NOT a str subclass: the render-path SafeStr check
        # lives on the cold branch (after PyUnicode_Check) so normal-str
        # rendering pays no overhead.
        self.assertNotIsInstance(SafeStr("x"), str)

    def test_wraps_value(self):
        self.assertEqual(SafeStr("<b>x</b>").s, "<b>x</b>")

    def test_repr(self):
        self.assertEqual(repr(SafeStr("<b>")), "SafeStr('<b>')")

    def test_non_str_arg_rejected(self):
        with self.assertRaises(TypeError):
            SafeStr(123)  # pyright: ignore[reportArgumentType]

    def test_content_verbatim(self):
        tpl = JSXTemplate("<div>", SLOT_VALUE, "</div>")
        self.assertEqual(
            tpl(SafeStr("<b>&amp;</b>")).to_html(),
            "<div><b>&amp;</b></div>",
        )

    def test_content_in_list(self):
        tpl = JSXTemplate("<div>", SLOT_VALUE, "</div>")
        self.assertEqual(
            tpl([SafeStr("<b>"), "a&b", SafeStr("</b>")]).to_html(),
            "<div><b>a&amp;b</b></div>",
        )

    def test_content_in_generator(self):
        tpl = JSXTemplate("<div>", SLOT_VALUE, "</div>")
        self.assertEqual(
            tpl(SafeStr(f"<i>{i}</i>") for i in range(3)).to_html(),
            "<div><i>0</i><i>1</i><i>2</i></div>",
        )

    def test_attr_instr_attr(self):
        tpl = JSXTemplate("<a", SlotAttr("href"), ">x</a>")
        self.assertEqual(
            tpl(SafeStr("/a?b&c\" class=\"foo")).to_html(),
            '<a href="/a?b&c" class="foo">x</a>',
        )

    def test_attr_spread(self):
        tpl = JSXTemplate("<a", SLOT_SPREAD, ">x</a>")
        self.assertEqual(
            tpl({"href": SafeStr("/a?b&c")}).to_html(),
            '<a href="/a?b&c">x</a>',
        )

    def test_spread_safe_str_as_key_rejected(self):
        # SafeStr is not a str, so it's rejected as an attribute name —
        # attr-name validation still requires a plain str key.
        tpl = JSXTemplate("<a", SLOT_SPREAD, ">x</a>")
        with self.assertRaises(TypeError):
            tpl({SafeStr("id"): "v"}).to_html()


class TestToHtmlDocument(unittest.TestCase):
    def test_default_prefix(self):
        tpl = JSXTemplate("<html><body>hi</body></html>")
        self.assertEqual(
            tpl().to_html_document(),
            "<!DOCTYPE html>\n<html><body>hi</body></html>",
        )

    def test_custom_prefix(self):
        tpl = JSXTemplate("<svg/>")
        self.assertEqual(
            tpl().to_html_document('<?xml version="1.0"?>\n'),
            '<?xml version="1.0"?>\n<svg/>',
        )

    def test_empty_prefix(self):
        tpl = JSXTemplate("<div>", SLOT_VALUE, "</div>")
        self.assertEqual(tpl("x").to_html_document(""), "<div>x</div>")

    def test_keyword_prefix(self):
        tpl = JSXTemplate("<html/>")
        self.assertEqual(
            tpl().to_html_document(prefix="X"),
            "X<html/>",
        )

    def test_prefix_not_escaped(self):
        # Prefix is emitted verbatim — it's not content, so HTML specials pass through.
        tpl = JSXTemplate("<html/>")
        self.assertEqual(
            tpl().to_html_document("<!-- & -->\n"),
            "<!-- & -->\n<html/>",
        )

    def test_non_str_prefix_rejected(self):
        tpl = JSXTemplate("<html/>")
        with self.assertRaises(TypeError):
            tpl().to_html_document(123)  # pyright: ignore[reportArgumentType]

    def test_renders_slots(self):
        tpl = JSXTemplate("<html><body>", SLOT_VALUE, "</body></html>")
        self.assertEqual(
            tpl("hi & bye").to_html_document(),
            "<!DOCTYPE html>\n<html><body>hi &amp; bye</body></html>",
        )


if __name__ == "__main__":
    unittest.main()
