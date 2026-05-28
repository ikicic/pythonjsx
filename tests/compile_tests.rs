//! Compiler tests — exercise the template-protocol output shape.
//!
//! Every JSX expression compiles to a `_pjr_tpl_N = _pjr_Tpl(...)` module-
//! level definition + a call-site `_pjr_tpl_N(args)`.  Runtime imports are
//! emitted conditionally based on which slot kinds appear, and
//! `assert_version` is the ABI guard (stripped from expected output by
//! `strip_abi_preamble` so individual tests focus on the JSX portion).

use pythonjsx::compiler::{compile, CompilerSettings};

fn normalize_whitespace(text: &str) -> String {
    let mut output = String::new();
    for line in text.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut collapsed = String::new();
        let mut in_space = false;
        for ch in line.chars() {
            if ch == ' ' || ch == '\t' {
                if !in_space {
                    collapsed.push(' ');
                    in_space = true;
                }
            } else {
                collapsed.push(ch);
                in_space = false;
            }
        }
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(collapsed.trim_end());
    }
    output
}

fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut min_indent = usize::MAX;
    for line in &lines {
        if line.trim().is_empty() { continue; }
        min_indent = min_indent.min(line.len() - line.trim_start().len());
    }
    if min_indent == usize::MAX { min_indent = 0; }
    lines.iter().enumerate().map(|(i, line)| {
        let prefix = if i > 0 { "\n" } else { "" };
        let content = if line.len() >= min_indent { &line[min_indent..] } else { line };
        format!("{}{}", prefix, content)
    }).collect()
}

/// Remove the ABI-version preamble from compiled output so that individual
/// compile tests can assert against just the JSX-compilation part they
/// care about.
fn strip_abi_preamble(text: &str) -> String {
    text.lines()
        .filter(|l| {
            !l.contains("assert_version as _pjr_assert_version")
                && !l.contains("_pjr_assert_version(")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse each `from pythonjsx.runtime import NAME as ALIAS` line and
/// verify the ALIAS appears at least once elsewhere in the output.  A
/// dead import means the compiler emitted a name it never references —
/// which pyright/LSP flags as a diagnostic on the generated file.
fn assert_all_imports_used(output: &str) {
    let prefix = "from pythonjsx.runtime import ";
    for line in output.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix(prefix) else { continue; };
        let alias = match rest.split_once(" as ") {
            Some((_, a)) => a.trim(),
            None => rest.trim(),
        };
        let n = output.matches(alias).count();
        assert!(
            n >= 2,
            "Import `{}` is emitted but never used in generated output:\n{}",
            alias,
            output,
        );
    }
}

fn test_compile(px_source: &str, expected: &str) {
    let px = dedent(px_source);
    let exp = dedent(expected);
    let (output, _sm, errors) = compile(&px, &CompilerSettings::default()).unwrap();
    assert!(errors.is_empty(), "Expected no errors but got: {:?}", errors);
    assert_all_imports_used(&output);
    let stripped = strip_abi_preamble(&output);
    let actual = normalize_whitespace(&stripped);
    let expected_norm = normalize_whitespace(&exp);
    assert_eq!(expected_norm, actual,
        "Compiled output mismatch.\nExpected:\n{}\n\nActual:\n{}", expected_norm, actual);
}

fn test_compile_error(px_source: &str, expected_msg: &str) {
    let px = dedent(px_source);
    let (output, _sm, errors) = compile(&px, &CompilerSettings::default()).unwrap();
    assert!(!errors.is_empty(), "Expected errors but got none. Output: {}", output);
    assert!(errors.iter().any(|e| e.message.contains(expected_msg)),
        "Expected error containing '{}' but got: {:?}", expected_msg, errors);
}

// ---------------------------------------------------------------------------
// Basic HTML tag compilation
// ---------------------------------------------------------------------------

#[test]
fn test_one_tag() {
    test_compile(
        r#"
        def App():
            return <div>Hello</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div>Hello</div>")()
        def App():
            return _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_one_tag_with_attribute() {
    test_compile(
        r#"
        def App():
            return <div id="header">Hello</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div id=\"header\">Hello</div>")()
        def App():
            return _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_one_tag_with_multiple_attributes() {
    test_compile(
        r#"
        def App():
            return <div id="header" class="container">Hello</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div id=\"header\" class=\"container\">Hello</div>")()
        def App():
            return _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_boolean_attribute_shorthand() {
    test_compile(
        r#"
        def foo():
            return <div bla foo="5"></div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div bla foo=\"5\"></div>")()
        def foo():
            return _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_nested_tags() {
    // Fully static nested tree collapses into one literal in a single-instr
    // template, then gets pre-evaluated at module load.
    test_compile(
        r#"
        def App():
            return <div><span>Hello</span></div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div><span>Hello</span></div>")()
        def App():
            return _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_nested_tags_with_attributes() {
    test_compile(
        r#"
        def Header():
            return <div id="header"><h1>Hello</h1></div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div id=\"header\"><h1>Hello</h1></div>")()
        def Header():
            return _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_mismatching_tags() {
    test_compile_error(
        r#"
        def App():
            return <div><span>Hello</div>
        "#,
        "tag",
    );
}

#[test]
fn test_invalid_attribute_value_syntax() {
    test_compile_error(
        r#"
        def foo():
            return <div a=5></div>
        "#,
        "Attribute value must be a string or expression",
    );
}

#[test]
fn test_no_import_when_no_jsx() {
    test_compile(
        r#"
        def hello():
            return "world"
        "#,
        r#"
        def hello():
            return "world"
        "#,
    );
}

#[test]
fn test_skip_docstring() {
    test_compile(
        r#"
        """Hello World"""
        def App():
            return <div>Hello World</div>
        "#,
        r#"
        """Hello World"""
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div>Hello World</div>")()
        def App():
            return _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_fragment() {
    // Fragment children get serialized as positional SLOT_VALUE args.  The
    // inter-component whitespace survives as single-space string chunks.
    // A static child (`test` inside `<C>`) is hoisted as an eager
    // module-level template whose `JSXResult` is passed by reference.
    test_compile(
        r#"
        def App():
            return <> <A /> <B /> <C id="test">test</C> </>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("test")()
        _pjr_tpl_1 = _pjr_Tpl(" ", _pjr_V, " ", _pjr_V, " ", _pjr_V, " ")
        def App():
            return _pjr_tpl_1(A(), B(), C(_pjr_tpl_0, id="test"))
        "#,
    );
}

#[test]
fn test_component_self_closing() {
    // A bare component call at the top level short-circuits: no template
    // is hoisted, the JSX element is replaced with the call expression
    // in-place.
    test_compile(
        r#"
        def App():
            return <Header title="x" />
        "#,
        r#"
        def App():
            return Header(title="x")
        "#,
    );
}

#[test]
fn test_spread_kwargs() {
    test_compile(
        r#"
        def App():
            kwargs = {'a': "hello", 'b': "world"}
            return <Component {**kwargs} />
        "#,
        r#"
        def App():
            kwargs = {'a': "hello", 'b': "world"}
            return Component(**kwargs)
        "#,
    );
}

// ---------------------------------------------------------------------------
// Import insertion
// ---------------------------------------------------------------------------

#[test]
fn test_no_import_when_all_jsx_in_error() {
    let src = dedent(
        r#"
        x = <div><span>foo</p></span></div>
        "#,
    );
    let (output, _sm, _errors) = compile(&src, &CompilerSettings::default()).unwrap();
    assert!(!output.contains("from pythonjsx.runtime"),
        "expected no pjr import:\n{}", output);
}

#[test]
fn test_import_present_when_valid_jsx_exists() {
    let src = dedent(
        r#"
        x = <div><span>foo</p></span></div>
        y = <div>ok</div>
        "#,
    );
    let (output, _sm, _errors) = compile(&src, &CompilerSettings::default()).unwrap();
    assert!(output.contains("from pythonjsx.runtime"),
        "expected pjr import:\n{}", output);
    assert!(output.contains("JSXTemplate"),
        "expected JSXTemplate import:\n{}", output);
}

// ---------------------------------------------------------------------------
// Control-flow shapes in function bodies
// ---------------------------------------------------------------------------

#[test]
fn test_if_elif_else_chain() {
    // Each branch hoists its own static template, pre-evaluated once at
    // module load.
    test_compile(
        r#"
        def Foo(x):
            if x > 0:
                return <div>a</div>
            elif x < 0:
                return <div>b</div>
            else:
                return <div>c</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div>a</div>")()
        _pjr_tpl_1 = _pjr_Tpl("<div>b</div>")()
        _pjr_tpl_2 = _pjr_Tpl("<div>c</div>")()
        def Foo(x):
            if x > 0:
                return _pjr_tpl_0
            elif x < 0:
                return _pjr_tpl_1
            else:
                return _pjr_tpl_2
        "#,
    );
}

#[test]
fn test_if_raise_then_return() {
    // Pre-return guard preserved verbatim; pure-static return becomes an
    // eager module-level template.
    test_compile(
        r#"
        def Foo(x):
            if x is None:
                raise ValueError()
            return <div>ok</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div>ok</div>")()
        def Foo(x):
            if x is None:
                raise ValueError()
            return _pjr_tpl_0
        "#,
    );
}

// ---------------------------------------------------------------------------
// Static-tree collapse + dynamic-chunk interleaving
// ---------------------------------------------------------------------------

#[test]
fn test_deep_static_tree_collapses_to_one_string() {
    test_compile(
        r#"
        x = <article class="post">
            <header>
                <h1 id="title">Hello</h1>
                <time>2024</time>
            </header>
            <p>Body here.</p>
        </article>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<article class=\"post\"><header><h1 id=\"title\">Hello</h1><time>2024</time></header><p>Body here.</p></article>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_void_elements_inside_static_tree() {
    test_compile(
        r#"
        x = <p>Line 1<br/>Line 2<br/>Line 3</p>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<p>Line 1<br/>Line 2<br/>Line 3</p>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_static_wraps_around_dynamic() {
    test_compile(
        r#"
        x = <div>Prefix <span>{name}</span> suffix</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<div>Prefix <span>", _pjr_V, "</span> suffix</div>")
        x = _pjr_tpl_0(name)
        "#,
    );
}

#[test]
fn test_component_calling_component() {
    test_compile(
        r#"
        def Icon():
            return <img src="icon.png" alt=""/>

        def Button(label: str):
            return <button><Icon /><span>{label}</span></button>

        x = <div><Button label="Click" /></div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<img src=\"icon.png\" alt=\"\"/>")()
        _pjr_tpl_1 = _pjr_Tpl("<button>", _pjr_V, "<span>", _pjr_V, "</span></button>")
        _pjr_tpl_2 = _pjr_Tpl("<div>", _pjr_V, "</div>")
        def Icon():
            return _pjr_tpl_0
        def Button(label: str):
            return _pjr_tpl_1(Icon(), label)
        x = _pjr_tpl_2(Button(label="Click"))
        "#,
    );
}

#[test]
fn test_boolean_and_static_attrs_mix() {
    test_compile(
        r#"
        x = <input type="checkbox" checked disabled value="on" />
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<input type=\"checkbox\" checked disabled value=\"on\"/>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_fragment_of_static_elements() {
    // Adjacent static elements in a fragment merge into one literal chunk.
    test_compile(
        r#"
        def Head():
            return <><title>My App</title><meta charset="utf-8"/></>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<title>My App</title><meta charset=\"utf-8\"/>")()
        def Head():
            return _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_dynamic_attr_blocks_full_collapse() {
    // A dynamic attr splits the opening tag; children still collapse but
    // the open portion becomes (pre, SlotAttr(name), post).
    test_compile(
        r#"
        x = <div class={cls}><h1>Static</h1><p>Also static</p></div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SlotAttr as _pjr_A
        _pjr_tpl_0 = _pjr_Tpl("<div", _pjr_A("class"), "><h1>Static</h1><p>Also static</p></div>")
        x = _pjr_tpl_0(cls)
        "#,
    );
}

#[test]
fn test_generator_expression_in_jsx() {
    // Inner JSX body (`<li>{i}</li>`) gets its own template; the outer JSX
    // passes the generator as a SLOT_VALUE (pjr_render_value iterates).
    test_compile(
        r#"
        items = ["a", "b", "c"]
        x = <ul>{<li>{i}</li> for i in items}</ul>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<li>", _pjr_V, "</li>")
        _pjr_tpl_1 = _pjr_Tpl("<ul>", _pjr_V, "</ul>")
        items = ["a", "b", "c"]
        x = _pjr_tpl_1((_pjr_tpl_0(i) for i in items))
        "#,
    );
}

#[test]
fn test_spread_content_list_splat() {
    // `{*args}` in content: the iterable passes as a single SLOT_VALUE
    // arg so pjr_render_value iterates at render time.  Emitting
    // `_pjr_tpl_N(*args)` would unpack at Python call time and mismatch
    // the template's slot count.
    test_compile(
        r#"
        args = ["a", "b", "c"]
        x = <div>Hello {*args}</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<div>Hello ", _pjr_V, "</div>")
        args = ["a", "b", "c"]
        x = _pjr_tpl_0(args)
        "#,
    );
}

#[test]
fn test_spread_content_multiple_in_one_jsx() {
    // Two separate `{*xs}` expressions → two SLOT_VALUE slots.
    test_compile(
        r#"
        x = <div>{*xs}{*ys}</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<div>", _pjr_V, _pjr_V, "</div>")
        x = _pjr_tpl_0(xs, ys)
        "#,
    );
}

#[test]
fn test_spread_content_with_complex_expression() {
    // The `*` must strip cleanly even when the expression isn't a bare name.
    test_compile(
        r#"
        x = <div>{*obj.items()}</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<div>", _pjr_V, "</div>")
        x = _pjr_tpl_0(obj.items())
        "#,
    );
}

#[test]
fn test_spread_child_of_component() {
    // `{*args}` as a component child splats at the Python call boundary
    // instead of being wrapped in a sub-template.
    test_compile(
        r#"
        x = <Foo>{*args}</Foo>
        "#,
        r#"
        x = Foo(*args)
        "#,
    );
}

#[test]
fn test_spread_child_of_component_mixed_with_regular_children() {
    // `{*args}` interleaves with regular positional children; splat
    // stays at its position in the arg list.  Single `{expr}` children
    // are passed raw (SLOT_VALUE escapes on the callee side).
    test_compile(
        r#"
        x = <Foo>{a}{*args}{b}</Foo>
        "#,
        r#"
        x = Foo(a, *args, b)
        "#,
    );
}

#[test]
fn test_static_attr_values_html_escaped_at_compile_time() {
    // `&` in a static attr value becomes `&amp;` baked into the literal.
    test_compile(
        r#"
        x = <a href="/a?b=1&amp;c=2" title="R&amp;D">ok</a>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<a href=\"/a?b=1&amp;c=2\" title=\"R&amp;D\">ok</a>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_self_closing_void_with_dynamic_attr() {
    test_compile(
        r#"
        x = <img src={url} alt="photo"/>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SlotAttr as _pjr_A
        _pjr_tpl_0 = _pjr_Tpl("<img", _pjr_A("src"), " alt=\"photo\"/>")
        x = _pjr_tpl_0(url)
        "#,
    );
}

#[test]
fn test_spread_between_static_attrs() {
    // `{**props}` splits the opening tag via SLOT_SPREAD; surrounding
    // static attrs bake into adjacent string chunks.
    test_compile(
        r#"
        x = <div id="x" {**props} class="y">ok</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_SPREAD as _pjr_S
        _pjr_tpl_0 = _pjr_Tpl("<div id=\"x\"", _pjr_S, " class=\"y\">ok</div>")
        x = _pjr_tpl_0(props)
        "#,
    );
}

#[test]
fn test_component_with_nested_dynamic_text() {
    test_compile(
        r#"
        def Box(x: int):
            return <div><h1>Title</h1><p>{x}</p></div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<div><h1>Title</h1><p>", _pjr_V, "</p></div>")
        def Box(x: int):
            return _pjr_tpl_0(x)
        "#,
    );
}

#[test]
fn test_component_returning_only_text_literal() {
    test_compile(
        r#"
        def Tag():
            return <span class="tag">new</span>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<span class=\"tag\">new</span>")()
        def Tag():
            return _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_multiple_dynamic_children_in_sequence() {
    // Three adjacent interpolations — each gets its own SLOT_VALUE marker.
    test_compile(
        r#"
        x = <div>{a}{b}{c}</div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<div>", _pjr_V, _pjr_V, _pjr_V, "</div>")
        x = _pjr_tpl_0(a, b, c)
        "#,
    );
}

#[test]
fn test_nested_static_component() {
    test_compile(
        r#"
        def Badge():
            return <span class="badge">new</span>

        def Card(title: str):
            return <div class="card"><h2>{title}</h2><Badge /></div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<span class=\"badge\">new</span>")()
        _pjr_tpl_1 = _pjr_Tpl("<div class=\"card\"><h2>", _pjr_V, "</h2>", _pjr_V, "</div>")
        def Badge():
            return _pjr_tpl_0
        def Card(title: str):
            return _pjr_tpl_1(title, Badge())
        "#,
    );
}

#[test]
fn test_bare_generator_in_jsx() {
    test_compile(
        r#"
        items = [1, 2, 3]
        result = <ul>{<li>{x}</li> for x in items}</ul>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<li>", _pjr_V, "</li>")
        _pjr_tpl_1 = _pjr_Tpl("<ul>", _pjr_V, "</ul>")
        items = [1, 2, 3]
        result = _pjr_tpl_1((_pjr_tpl_0(x) for x in items))
        "#,
    );
}

#[test]
fn test_component_looked_up_from_nested_expression() {
    test_compile(
        r#"
        def Badge():
            return <span class="badge">!</span>

        def Card(title: str):
            return <div>{title.upper()}<Badge/></div>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        from pythonjsx.runtime import SLOT_VALUE as _pjr_V
        _pjr_tpl_0 = _pjr_Tpl("<span class=\"badge\">!</span>")()
        _pjr_tpl_1 = _pjr_Tpl("<div>", _pjr_V, _pjr_V, "</div>")
        def Badge():
            return _pjr_tpl_0
        def Card(title: str):
            return _pjr_tpl_1(title.upper(), Badge())
        "#,
    );
}

// ---------------------------------------------------------------------------
// HTML5 void-element awareness
// ---------------------------------------------------------------------------
//
// HTML5 defines a closed set of *void* elements (br, hr, img, input, link,
// meta, …) that cannot have children or end tags.  Everything else is
// *non-void*: it must have explicit `<open>…</open>` form even when empty,
// because `<div/>` is invalid HTML5 (the `/` is ignored and the rest of
// the document becomes a child of the div).
//
// The compiler is HTML-aware here and produces valid HTML:
//   * `<div/>` → `<div></div>` (expand non-void self-close)
//   * `<br/>`  → `<br/>` (already valid — HTML5 permits `/` on void tags)
//   * `<br></br>` → compile error (void elements cannot have end tags)

#[test]
fn test_non_void_self_close_expands_to_open_close() {
    test_compile(
        r#"
        x = <div/>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div></div>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_non_void_self_close_with_attrs_expands_to_open_close() {
    test_compile(
        r#"
        x = <span class="badge"/>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<span class=\"badge\"></span>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_void_self_close_preserved() {
    // <br/> is valid HTML5 for a void element — keep as-is.
    test_compile(
        r#"
        x = <br/>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<br/>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_void_with_end_tag_is_error() {
    test_compile_error(
        r#"
        x = <br></br>
        "#,
        "void element",
    );
}

#[test]
fn test_void_with_explicit_children_is_error() {
    test_compile_error(
        r#"
        x = <img>text</img>
        "#,
        "void element",
    );
}

// ---------------------------------------------------------------------------
// camelCase tag names — SVG elements like <linearGradient>, <clipPath>,
// <feGaussianBlur>.  The grammar accepts them as tag names (starting
// lowercase → not a component) and the emitter preserves the exact
// casing.  SVG elements that aren't HTML5-void still expand self-close
// into open/close (e.g. `<circle r="5"/>` → `<circle r="5"></circle>`).
// ---------------------------------------------------------------------------

#[test]
fn test_svg_camelcase_tag_name() {
    test_compile(
        r#"
        x = <linearGradient id="g"></linearGradient>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<linearGradient id=\"g\"></linearGradient>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_svg_nested_camelcase() {
    test_compile(
        r#"
        x = <svg><clipPath id="c"><circle r="5"/></clipPath></svg>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<svg><clipPath id=\"c\"><circle r=\"5\"></circle></clipPath></svg>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_svg_self_closing_camelcase() {
    test_compile(
        r#"
        x = <feGaussianBlur stdDeviation="2"/>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<feGaussianBlur stdDeviation=\"2\"></feGaussianBlur>")()
        x = _pjr_tpl_0
        "#,
    );
}

#[test]
fn test_underscore_capitalized_name_is_component() {
    test_compile(
        r#"
        x = <_Capitalized label="ok" />
        "#,
        r#"
        x = _Capitalized(label="ok")
        "#,
    );
}

#[test]
fn test_underscore_lowercase_name_is_component() {
    test_compile(
        r#"
        x = <_noncapitalized label="ok" />
        "#,
        r#"
        x = _noncapitalized(label="ok")
        "#,
    );
}

// ---------------------------------------------------------------------------
// ABI-version preamble
// ---------------------------------------------------------------------------

#[test]
fn test_abi_preamble_emitted_for_jsx() {
    // Every compiled module with JSX imports `assert_version` and calls it
    // once at the top — this is what catches a compiler/runtime mismatch at
    // module import rather than at first render.
    let px = "x = <div>hi</div>\n";
    let (output, _sm, errors) = compile(px, &CompilerSettings::default()).unwrap();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert!(
        output.contains("from pythonjsx.runtime import assert_version as _pjr_assert_version"),
        "expected ABI-version import in output:\n{}",
        output,
    );
    assert!(
        output.contains("_pjr_assert_version("),
        "expected ABI-version call in output:\n{}",
        output,
    );
}

#[test]
fn test_abi_preamble_not_emitted_without_jsx() {
    // Pure Python — no runtime dependency, no preamble needed.
    let px = "x = 1\n";
    let (output, _sm, _errors) = compile(px, &CompilerSettings::default()).unwrap();
    assert!(
        !output.contains("_pjr_assert_version"),
        "pure Python shouldn't get a runtime-dependency preamble:\n{}",
        output,
    );
}

// ---------------------------------------------------------------------------
// Duplicate component-name rejection
// ---------------------------------------------------------------------------

#[test]
fn test_duplicate_component_names_error() {
    test_compile_error(
        r#"
        def Foo():
            return <div>a</div>

        def Foo():
            return <span>b</span>
        "#,
        "Duplicate component",
    );
}

// ---------------------------------------------------------------------------
// Component children — lowering of static / dynamic / spread children.
// ---------------------------------------------------------------------------

#[test]
fn test_static_component_child_hoisted_to_eager_template() {
    // A fully-static JSX child is materialised as a module-level eager
    // `_pjr_Tpl(...)()` and passed to the component by reference — one
    // `JSXResult` allocation at import, zero per-call.  Without this
    // the bare HTML would be passed as a Python str and re-escaped.
    test_compile(
        r#"
        def App():
            return <Foo><div>Hello</div></Foo>
        "#,
        r#"
        from pythonjsx.runtime import JSXTemplate as _pjr_Tpl
        _pjr_tpl_0 = _pjr_Tpl("<div>Hello</div>")()
        def App():
            return Foo(_pjr_tpl_0)
        "#,
    );
}

#[test]
fn test_dynamic_component_child_passed_raw() {
    // `{expr}` as a component child is passed raw — SLOT_VALUE on the
    // component's side handles the HTML escape.  No sub-template.
    test_compile(
        r#"
        x = <Foo>{a}</Foo>
        "#,
        r#"
        x = Foo(a)
        "#,
    );
}
