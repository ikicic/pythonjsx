//! Visitor that drives the Emitter to compile .px to .py.

use crate::compiler::chunks::{as_template_call, Chunk, Chunks};
use crate::compiler::emitter::Emitter;
use crate::compiler::html_entities::{
    collapse_jsx_whitespace, decode_html_entities, decode_html_entities_with_unknowns,
};
use crate::compiler::opcodes::{
    OP_ESCAPE_TEXT, OP_RENDER_ATTR, OP_UNPACK_ARGS, OP_UNPACK_ATTRS, RUNTIME_ABI_VERSION,
};
use crate::compiler::settings::CompilerSettings;
use crate::compiler::sourcemap::SourceMap;
use crate::parser;
use std::str;
use tree_sitter::{Node, Tree};

/// Materialize children into a Vec; `node.child(i)` is not O(1).
fn collect_children<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut out: Vec<Node<'a>> = Vec::with_capacity(node.child_count());
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            out.push(cursor.node());
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    out
}

fn for_each_child<'a, F: FnMut(Node<'a>)>(node: Node<'a>, mut f: F) {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            f(cursor.node());
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// Python hard keywords (excludes soft keywords `match`, `case`, `type`, `_`,
// which can legally appear as attribute names).
static PYTHON_HARD_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// Capitalized names (e.g. Header, MyComponent) and names starting with
/// underscore are components; all other names are tags.
fn is_component(tag_name: &str) -> bool {
    tag_name.starts_with('_')
        || tag_name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
}

/// Whether `name` can be emitted bare as a Python kwarg.  ASCII-only
/// because Rust's `is_alphanumeric` is a superset of XID_Continue (e.g.
/// `²`, `½`), which produces SyntaxError in emitted .py.  Rejected names
/// fall through to the `**{"name": value}` emission path.
fn valid_python_kwarg(name: &str) -> bool {
    if PYTHON_HARD_KEYWORDS.contains(&name) {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Classify ERROR node content and return (error_message, placeholder_for_output).
fn classify_error_node(text: &str) -> Option<(&'static str, &'static str)> {
    let t = text.trim();
    // Orphan closing tag: </tagname>
    if t.starts_with("</") && t.ends_with('>') && t.len() >= 4 {
        let inner = &t[2..t.len() - 1];
        let mut chars = inner.chars();
        if matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
            && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Some(("Unexpected closing tag", "None"));
        }
    }
    // Fragment with wrong closing: <>...</X> where X != ""
    if t.starts_with("<>") && t.contains("</") && !t.ends_with("</>") {
        return Some(("Fragment must close with </>", "None"));
    }
    // Element with fragment close: <tagname>...</> (not valid fragment)
    if t.len() > 4
        && t.starts_with('<')
        && !t.starts_with("<>")
        && t.contains('>')
        && t.ends_with("</>")
    {
        return Some(("Expected closing tag to match opening, not </>", "None"));
    }
    // Spread missing **: {expr} in attribute position
    if t.starts_with('{') && !t.starts_with("{**") && t.ends_with('}') {
        return Some(("Spread attribute requires **: use {**expr}", "**{}"));
    }
    None
}

fn escape_string(s: &str) -> String {
    format!("{:?}", s)
}

/// Warn for entity-shaped `&name;` sequences that don't match any known
/// HTML entity.  `base` is the source offset of `text` (for diagnostics).
fn warn_on_unknown_entities(emitter: &mut Emitter, text: &str, base: usize) {
    let (_decoded, unknowns) = decode_html_entities_with_unknowns(text);
    for (lo, hi) in unknowns {
        let name = &text[lo..hi];
        emitter.warning(
            format!(
                "Unknown named HTML entity {}: check the spelling, or write '&amp;' for a literal '&'",
                name
            ),
            base + lo,
            base + hi,
        );
    }
}

/// Trim ASCII whitespace off the ends of a byte range so diagnostics don't
/// land on the incidental whitespace tree-sitter folds into ERROR nodes.
fn trim_ws_range(source: &[u8], start: usize, end: usize) -> (usize, usize) {
    let mut s = start;
    let mut e = end;
    while s < e && (source[s] as char).is_ascii_whitespace() {
        s += 1;
    }
    while e > s && (source[e - 1] as char).is_ascii_whitespace() {
        e -= 1;
    }
    (s, e)
}

/// Diagnose a `jsx_element` whose closing tag name differs from its opening tag.
fn jsx_element_close_mismatch(node: Node, source: &[u8]) -> Option<(String, usize, usize)> {
    debug_assert_eq!(node.kind(), "jsx_element");
    let tag_name_node = find_tag_name_node(node)?;
    let tag_name =
        str::from_utf8(&source[tag_name_node.start_byte()..tag_name_node.end_byte()]).ok()?;

    let cc = node.child_count();
    if cc < 3 {
        return None;
    }
    let a = node.child((cc - 3) as u32)?;
    let b = node.child((cc - 2) as u32)?;
    let c = node.child((cc - 1) as u32)?;
    let a_text = str::from_utf8(&source[a.start_byte()..a.end_byte()]).ok()?;
    let c_text = str::from_utf8(&source[c.start_byte()..c.end_byte()]).ok()?;
    let is_slash = a.kind() == "</" || a_text == "</";
    let is_name = b.kind() == "identifier" || b.kind() == "jsx_tag_name";
    let is_angle = c.kind() == ">" || c_text == ">";
    if !(is_slash && is_name && is_angle) {
        return None;
    }
    let end_tag = str::from_utf8(&source[b.start_byte()..b.end_byte()]).ok()?;
    if end_tag == tag_name {
        return None;
    }
    Some((
        format!("Expected tag name '{}', got '{}'", tag_name, end_tag),
        b.start_byte(),
        b.end_byte(),
    ))
}

/// Collect close-tag mismatches from every nested `jsx_element` inside
/// an ERROR-wrapped expression so the diagnostic points at the deeper mistake.
fn collect_nested_mismatches(
    node: Node,
    source: &[u8],
    out: &mut Vec<(String, usize, usize)>,
) {
    if node.kind() == "jsx_element" {
        if let Some(m) = jsx_element_close_mismatch(node, source) {
            out.push(m);
        }
    }
    for_each_child(node, |child| {
        collect_nested_mismatches(child, source, out);
    });
}

/// Inspect the immediate children of an ERROR node and try to recognise a
/// specific malformed-JSX shape. Returns a precise (message, start, end) if a
/// pattern matches, otherwise None.
fn scan_error_shape(node: Node, source: &[u8]) -> Option<(String, usize, usize)> {
    debug_assert_eq!(node.kind(), "ERROR");

    let mut cs: Vec<(&'static str, usize, usize)> = Vec::new();
    for_each_child(node, |c| {
        cs.push((c.kind(), c.start_byte(), c.end_byte()));
    });

    // Skip Python prefix children before the first JSX-looking token.
    let jsx_start = cs.iter().position(|(k, _, _)| {
        matches!(
            *k,
            "<" | "<>" | "</" | "jsx_element" | "jsx_fragment" | "jsx_tag_name"
        )
    })?;
    let mut tail: Vec<(&'static str, usize, usize)> = cs[jsx_start..].to_vec();
    // Strip trailing jsx_text — tree-sitter absorbs post-error text as jsx_text.
    while matches!(tail.last(), Some((k, _, _)) if *k == "jsx_text") {
        tail.pop();
    }

    // Orphan closing tag: `<` ERROR(/) name `>` or `</` name `>`.
    let orphan_end_idx_and_span = {
        if tail.len() >= 4
            && tail[0].0 == "<"
            && tail[1].0 == "ERROR"
            && tail[2].0 == "jsx_tag_name"
            && tail[3].0 == ">"
        {
            let err_text = str::from_utf8(&source[tail[1].1..tail[1].2])
                .unwrap_or("")
                .trim();
            if err_text == "/" {
                Some((4usize, tail[0].1, tail[3].2))
            } else {
                None
            }
        } else if tail.len() >= 3
            && tail[0].0 == "</"
            && tail[1].0 == "jsx_tag_name"
            && tail[2].0 == ">"
        {
            Some((3usize, tail[0].1, tail[2].2))
        } else {
            None
        }
    };
    if let Some((consumed, start, end)) = orphan_end_idx_and_span {
        // Only an orphan if nothing JSX-ish follows (trailing jsx_text is fine).
        let rest_ok = tail[consumed..]
            .iter()
            .all(|(k, _, _)| matches!(*k, "jsx_text"));
        if rest_ok {
            return Some(("Unexpected closing tag".to_string(), start, end));
        }
    }

    let find_seq = |from: usize, kinds: &[&str]| -> Option<usize> {
        if kinds.is_empty() || from + kinds.len() > tail.len() {
            return None;
        }
        (from..=tail.len() - kinds.len()).find(|&i| {
            kinds
                .iter()
                .enumerate()
                .all(|(k, want)| tail[i + k].0 == *want)
        })
    };

    // Fragment opened, element closed.
    if tail[0].0 == "<>" {
        if let Some(i) = find_seq(1, &["<", "ERROR", "jsx_tag_name", ">"]) {
            let err = tail[i + 1];
            let err_text = str::from_utf8(&source[err.1..err.2]).unwrap_or("").trim();
            if err_text == "/" {
                return Some((
                    "Fragment must close with </>".to_string(),
                    tail[i].1,
                    tail[i + 3].2,
                ));
            }
        }
        if let Some(i) = find_seq(1, &["</", "jsx_tag_name", ">"]) {
            return Some((
                "Fragment must close with </>".to_string(),
                tail[i].1,
                tail[i + 2].2,
            ));
        }
    }

    // Element opened, fragment closed.
    if tail.len() >= 5 && tail[0].0 == "<" && tail[1].0 == "jsx_tag_name" && tail[2].0 == ">"
    {
        if let Some(i) = find_seq(3, &["</", ">"]) {
            let tag_text =
                str::from_utf8(&source[tail[1].1..tail[1].2]).unwrap_or("<?>");
            return Some((
                format!(
                    "Expected closing tag '</{}>', got '</>'",
                    tag_text
                ),
                tail[i].1,
                tail[i + 1].2,
            ));
        }
    }

    // Malformed opening tag (`<name` with non-attribute junk).
    if tail.len() >= 4
        && tail[0].0 == "<"
        && tail[1].0 == "jsx_tag_name"
        && tail[2].0 == "ERROR"
    {
        let tag_text =
            str::from_utf8(&source[tail[1].1..tail[1].2]).unwrap_or("<?>");
        return Some((
            format!(
                "Malformed opening tag '<{}': expected attributes or '>'",
                tag_text
            ),
            tail[0].1,
            tail[1].2,
        ));
    }

    // Unclosed element: `<` name `>` ... with no `</name>` terminator.
    if tail.len() >= 3 && tail[0].0 == "<" && tail[1].0 == "jsx_tag_name" && tail[2].0 == ">"
    {
        let has_close = tail[3..].iter().any(|(k, _, _)| *k == "</");
        if !has_close {
            let tag_text =
                str::from_utf8(&source[tail[1].1..tail[1].2]).unwrap_or("<?>");
            return Some((
                format!("Unclosed element '<{}>'", tag_text),
                tail[0].1,
                tail[2].2,
            ));
        }
    }

    None
}

fn scan_for_jsx(tree: &Tree) -> bool {
    scan_node(tree.root_node())
}

fn scan_node(node: Node) -> bool {
    if node.kind() == "jsx_element" || node.kind() == "jsx_fragment" {
        return true;
    }
    // Skip ERROR subtrees: their JSX is emitted as `None`, so it doesn't
    // require the runtime import (avoids spurious "unused import" warnings).
    if node.kind() == "ERROR" {
        return false;
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            if scan_node(cursor.node()) {
                return true;
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    false
}

fn should_skip_module_child(kind: &str, first_child_kind: Option<&str>) -> bool {
    match kind {
        "comment" => true,
        "future_import_statement" => true,
        "expression_statement" => first_child_kind == Some("string"),
        _ => false,
    }
}

/// Visit a node into a fresh sub-emitter and return its output as a string.
/// Module-wide state (template counter, defs, slot-usage flags) is threaded
/// through the parent so sub-visits remain in module scope.
fn visit_to_string(
    tree: &Tree,
    node: Node,
    source: &[u8],
    settings: &CompilerSettings,
    parent_emitter: &mut Emitter,
) -> String {
    let mut emitter = Emitter::new(source);
    emitter.template_counter = parent_emitter.template_counter;
    visit_inner(tree, node, &mut emitter, settings);
    parent_emitter.merge_errors(std::mem::take(&mut emitter.errors));
    parent_emitter.template_counter = emitter.template_counter;
    parent_emitter
        .template_defs
        .push_str(&std::mem::take(&mut emitter.template_defs));
    parent_emitter.uses_slot_value |= emitter.uses_slot_value;
    parent_emitter.uses_slot_spread |= emitter.uses_slot_spread;
    parent_emitter.uses_slot_attr |= emitter.uses_slot_attr;
    str::from_utf8(&emitter.output).unwrap_or("").to_string()
}

/// Compile .px source to .py. Returns (output, source_map, errors).
pub fn compile(
    source: &str,
    settings: &CompilerSettings,
) -> Result<
    (
        String,
        SourceMap,
        Vec<crate::compiler::error::CompileError>,
    ),
    tree_sitter::LanguageError,
> {
    let tree = parser::parse(source)?;
    let source_bytes = source.as_bytes();
    let has_jsx = scan_for_jsx(&tree);
    let root = tree.root_node();
    let mut emitter = Emitter::new(source_bytes);
    visit_module(&tree, root, &mut emitter, settings, has_jsx);
    let output = String::from_utf8_lossy(&emitter.output).to_string();
    let (source_map_root, errors) = emitter.finish();
    Ok((output, SourceMap::new(source_map_root), errors))
}

fn html_escape_attr(s: &str) -> String {
    if !s.bytes().any(|b| matches!(b, b'&' | b'<' | b'>' | b'"')) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn html_escape_text(s: &str) -> String {
    if !s.bytes().any(|b| matches!(b, b'&' | b'<' | b'>')) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}
fn visit_module(
    tree: &Tree,
    node: Node,
    emitter: &mut Emitter,
    settings: &CompilerSettings,
    has_jsx: bool,
) {
    let source = emitter.source;
    // Track top-level component `def`s so duplicates raise a clear error.
    let mut seen_component_names: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    let mut pos = node.start_byte();
    let mut inserted_import = false;
    let module_children = collect_children(node);
    for child in module_children {
        let kind = child.kind();
        let first_child_kind = child.child(0).map(|c| c.kind());
        emitter.emit_verbatim(pos, child.start_byte());
        if !inserted_import && !should_skip_module_child(kind, first_child_kind.as_deref()) {
            if has_jsx {
                // Mark where the preamble (imports + ABI assert + template
                // defs) will be spliced at module end — before any user
                // code, so import-time expressions see the templates.
                emitter.template_insert_pos = Some(emitter.output.len());
            }
            inserted_import = true;
        }
        emitter.open_region(child.start_byte(), child.end_byte());
        visit_inner(tree, child, emitter, settings);
        emitter.close_region();

        if kind == "function_definition" {
            // Reject duplicate component names.
            if let Some(name_node) = child.child_by_field_name("name") {
                let name_text = str::from_utf8(
                    &source[name_node.start_byte()..name_node.end_byte()],
                )
                .unwrap_or("");
                if is_component(name_text) {
                    if let Some(&(prev_s, prev_e)) = seen_component_names.get(name_text) {
                        let _ = (prev_s, prev_e); // reserved for a "previously defined at …" note
                        emitter.error(
                            format!(
                                "Duplicate component '{}' (a component with this name was already defined in this module)",
                                name_text
                            ),
                            name_node.start_byte(),
                            name_node.end_byte(),
                        );
                        pos = child.end_byte();
                        continue;
                    }
                    seen_component_names.insert(
                        name_text.to_string(),
                        (name_node.start_byte(), name_node.end_byte()),
                    );
                }
            }
        }

        pos = child.end_byte();
    }
    emitter.emit_verbatim(pos, node.end_byte());
    // Splice the preamble at `template_insert_pos`.  Imports are gated on
    // actual use (`uses_slot_*` flags + `template_defs` non-emptiness) to
    // keep generated code free of "unused import" diagnostics.
    if has_jsx {
        if let Some(at) = emitter.template_insert_pos {
            let defs = std::mem::take(&mut emitter.template_defs);
            let mut preamble = String::new();
            preamble.push('\n');
            let mut push_import = |name: &str, alias: &str| {
                preamble.push_str(&format!(
                    "from pythonjsx.runtime import {} as {}\n",
                    name, alias,
                ));
            };
            if !defs.is_empty() {
                push_import("JSXTemplate", "_pjr_Tpl");
            }
            if emitter.uses_slot_value {
                push_import("SLOT_VALUE", "_pjr_V");
            }
            if emitter.uses_slot_spread {
                push_import("SLOT_SPREAD", "_pjr_S");
            }
            if emitter.uses_slot_attr {
                push_import("SlotAttr", "_pjr_A");
            }
            push_import("assert_version", "_pjr_assert_version");
            preamble.push_str(&format!(
                "_pjr_assert_version({})\n",
                RUNTIME_ABI_VERSION,
            ));
            preamble.push_str(&defs);
            emitter.splice_in(at, preamble.as_bytes());
        }
    }
}

fn visit_inner(
    tree: &Tree,
    node: Node,
    emitter: &mut Emitter,
    settings: &CompilerSettings,
) {
    let source = emitter.source;
    let kind = node.kind();

    match kind {
        "jsx_element" => visit_jsx_element(tree, node, emitter, settings),
        "jsx_fragment" => visit_jsx_fragment(tree, node, emitter, settings),
        "jsx_text" => {
            let raw = str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("");
            warn_on_unknown_entities(emitter, raw, node.start_byte());
            if let Some(collapsed) = collapse_jsx_whitespace(raw) {
                let decoded = decode_html_entities(&collapsed);
                // HTML-escape at compile time so the runtime can trust `str`.
                let html_escaped = decoded
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                emitter.emit_generated(
                    node.start_byte(),
                    node.end_byte(),
                    &escape_string(&html_escaped),
                );
            }
        }
        "jsx_expression" => {
            // Emit the inner expression (skip the surrounding { }).
            let mut cursor = node.walk();
            if cursor.goto_first_child() {
                loop {
                    let child = cursor.node();
                    if child.kind() != "{" && child.kind() != "}" {
                        visit_inner(tree, child, emitter, settings);
                        break;
                    }
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
        "jsx_generator_expression" => {
            // Bare generator `{expr for x in y}` → `(expr for x in y)`.
            emitter.open_region(node.start_byte(), node.end_byte());
            let mut prev_end = node.start_byte();
            let mut cursor = node.walk();
            if cursor.goto_first_child() {
                loop {
                    let child = cursor.node();
                    if child.kind() == "{" {
                        emitter.emit_generated(child.start_byte(), child.end_byte(), "(");
                        prev_end = child.end_byte();
                    } else if child.kind() == "}" {
                        emitter.emit_generated(prev_end, child.end_byte(), ")");
                        break;
                    } else {
                        emitter.emit_verbatim(prev_end, child.start_byte());
                        emitter.open_region(child.start_byte(), child.end_byte());
                        visit_inner(tree, child, emitter, settings);
                        emitter.close_region();
                        prev_end = child.end_byte();
                    }
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
            emitter.close_region();
        }
        "ERROR" => {
            let err_text =
                str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("");
            let (trim_start, trim_end) =
                trim_ws_range(source, node.start_byte(), node.end_byte());

            // Prefer reporting deeper close-tag mismatches over the outer
            // ERROR shape.  This path and the per-element check in
            // `visit_jsx_element` are mutually exclusive (regression test
            // in tests/error_position_tests.rs).
            let mut nested = Vec::new();
            collect_nested_mismatches(node, source, &mut nested);
            if !nested.is_empty() {
                for (msg, s, e) in nested {
                    emitter.error(msg, s, e);
                }
                emitter.emit_generated(node.start_byte(), node.end_byte(), "None");
            } else if let Some((msg, s, e)) = scan_error_shape(node, source) {
                emitter.error(msg, s, e);
                emitter.emit_generated(node.start_byte(), node.end_byte(), "None");
            } else if let Some((msg, placeholder)) = classify_error_node(err_text) {
                emitter.error(msg, trim_start, trim_end);
                emitter.emit_generated(node.start_byte(), node.end_byte(), placeholder);
            } else if err_text.contains('<') && err_text.contains('>') {
                // JSX-like ERROR with no specific pattern match — generic message.
                emitter.error(
                    "Invalid JSX syntax (e.g. mismatched tags <div>...</span>)",
                    trim_start,
                    trim_end,
                );
                emitter.emit_generated(node.start_byte(), node.end_byte(), "None");
            } else {
                // Unknown ERROR — placeholder to keep Python parseable.
                emitter.emit_generated(node.start_byte(), node.end_byte(), "None");
            }
        }
        _ => {
            let mut pos = node.start_byte();
            for_each_child(node, |child| {
                emitter.emit_verbatim(pos, child.start_byte());
                emitter.open_region(child.start_byte(), child.end_byte());
                visit_inner(tree, child, emitter, settings);
                emitter.close_region();
                pos = child.end_byte();
            });
            emitter.emit_verbatim(pos, node.end_byte());
        }
    }
}

fn find_tag_name_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            let k = child.kind();
            if k == "_jsx_element_name" || k == "identifier" || k == "jsx_tag_name" {
                return Some(child);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

/// Classification of a JSX attribute, so static attrs can be baked into
/// the opening tag string and dynamic ones routed through render slots.
enum AttrKind {
    StaticKV { name: String, value: String },
    DynamicKV { name: String, expr: String },
    Boolean { name: String },
    Spread { expr: String },
}

/// Compile a `jsx_element` or `jsx_fragment` into a `Chunks` sequence.
fn compile_jsx_to_chunks(
    tree: &Tree,
    node: Node,
    emitter: &mut Emitter,
    settings: &CompilerSettings,
) -> Chunks {
    match node.kind() {
        "jsx_element" => compile_jsx_element_to_chunks(tree, node, emitter, settings),
        "jsx_fragment" => compile_jsx_fragment_to_chunks(tree, node, emitter, settings),
        _ => {
            let mut c = Chunks::new();
            let src_text = visit_to_string(tree, node, emitter.source, settings, emitter);
            c.push_dynamic(src_text);
            c
        }
    }
}

fn compile_jsx_element_to_chunks(
    tree: &Tree,
    node: Node,
    emitter: &mut Emitter,
    settings: &CompilerSettings,
) -> Chunks {
    let source = emitter.source;

    let tag_name_node = match find_tag_name_node(node) {
        Some(n) => n,
        None => {
            emitter.error("No tag name found", node.start_byte(), node.end_byte());
            let mut c = Chunks::new();
            c.push_dynamic("_pjr_JSXNode(\"<div></div>\")".to_string());
            return c;
        }
    };
    let tag_name =
        str::from_utf8(&source[tag_name_node.start_byte()..tag_name_node.end_byte()])
            .unwrap_or("")
            .to_string();

    let mut attrs_kwargs: Vec<String> = Vec::new();
    let mut attr_kinds: Vec<AttrKind> = Vec::new();
    let mut child_chunks_list: Vec<Chunks> = Vec::new();
    let mut is_self_closing = false;
    let mut skip_idx: Option<usize> = None;
    let mut seen_attr_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    let check_dup = |name: &str,
                     range: (usize, usize),
                     emitter: &mut Emitter,
                     seen: &mut std::collections::HashSet<String>|
     -> bool {
        if seen.contains(name) {
            emitter.error(format!("Duplicate attribute '{}'", name), range.0, range.1);
            true
        } else {
            seen.insert(name.to_string());
            false
        }
    };

    let children = collect_children(node);
    for i in 0..children.len() {
        if Some(i) == skip_idx {
            continue;
        }
        let child = children[i];
        match child.kind() {
            "jsx_attribute" => {
                let (attr_name, attr_value, raw_decoded) =
                    visit_jsx_attribute_to_string(tree, child, source, settings, emitter);
                if attr_name.is_empty() {
                    let (s, e) =
                        trim_ws_range(source, child.start_byte(), child.end_byte());
                    emitter.error("Missing attribute name", s, e);
                } else {
                    let name_node = child
                        .child(0)
                        .filter(|c| c.kind() == "jsx_attribute_name")
                        .unwrap_or(child);
                    let is_dup = check_dup(
                        &attr_name,
                        (name_node.start_byte(), name_node.end_byte()),
                        emitter,
                        &mut seen_attr_names,
                    );
                    if !is_dup {
                        let attr_str = if valid_python_kwarg(&attr_name) {
                            format!("{}={}", attr_name, attr_value)
                        } else {
                            format!("**{{{:?}: {}}}", attr_name, attr_value)
                        };
                        match raw_decoded {
                            Some(decoded) => attr_kinds.push(AttrKind::StaticKV {
                                name: attr_name.clone(),
                                value: decoded,
                            }),
                            None => attr_kinds.push(AttrKind::DynamicKV {
                                name: attr_name.clone(),
                                expr: attr_value.clone(),
                            }),
                        }
                        attrs_kwargs.push(attr_str);
                    }
                }
            }
            "jsx_boolean_attribute" => {
                let next_idx = i + 1;
                let followed_by_invalid_value =
                    if let Some(&next_child) = children.get(next_idx) {
                        if next_child.kind() == "ERROR" {
                            let next_text = str::from_utf8(
                                &source[next_child.start_byte()..next_child.end_byte()],
                            )
                            .unwrap_or("");
                            next_text.trim_start().starts_with('=')
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                if followed_by_invalid_value {
                    let error_end = children
                        .get(next_idx)
                        .map(|n| n.end_byte())
                        .unwrap_or(child.end_byte());
                    emitter.error(
                        "Attribute value must be a string or expression: use name=\"value\" or name={expr}",
                        child.start_byte(),
                        error_end,
                    );
                    skip_idx = Some(next_idx);
                } else {
                    let attr_name =
                        str::from_utf8(&source[child.start_byte()..child.end_byte()])
                            .unwrap_or("");
                    let is_dup = check_dup(
                        attr_name,
                        (child.start_byte(), child.end_byte()),
                        emitter,
                        &mut seen_attr_names,
                    );
                    if !is_dup {
                        let attr_str = if valid_python_kwarg(attr_name) {
                            format!("{}=True", attr_name)
                        } else {
                            format!("**{{{:?}: True}}", attr_name)
                        };
                        attr_kinds.push(AttrKind::Boolean {
                            name: attr_name.to_string(),
                        });
                        attrs_kwargs.push(attr_str);
                    }
                }
            }
            "jsx_spread_attribute" => {
                let spread = visit_jsx_spread_attribute_to_string(
                    tree, child, emitter, source, settings,
                );
                let spread_expr = if spread.starts_with("**") {
                    spread[2..].to_string()
                } else {
                    spread.clone()
                };
                attr_kinds.push(AttrKind::Spread { expr: spread_expr });
                attrs_kwargs.push(spread);
            }
            "ERROR" => {
                let err_text =
                    str::from_utf8(&source[child.start_byte()..child.end_byte()])
                        .unwrap_or("");
                let trimmed = err_text.trim_start();
                let (err_start, err_end) =
                    trim_ws_range(source, child.start_byte(), child.end_byte());
                if trimmed.starts_with('{') && !trimmed.starts_with("{**") {
                    emitter.error(
                        "Spread attribute requires **: use {**expr}",
                        err_start,
                        err_end,
                    );
                    attr_kinds.push(AttrKind::Spread {
                        expr: "{}".to_string(),
                    });
                    attrs_kwargs.push("**{}".to_string());
                } else if trimmed.starts_with('=') {
                    emitter.error("Missing attribute name", err_start, err_end);
                } else if err_start < err_end {
                    emitter.error(
                        format!(
                            "Unexpected token {:?} inside <{}>",
                            err_text.trim(),
                            tag_name
                        ),
                        err_start,
                        err_end,
                    );
                }
            }
            "_jsx_child" => {
                for_each_child(child, |grandchild| {
                    if let Some(ch) =
                        compile_child_to_chunks(tree, grandchild, emitter, settings)
                    {
                        child_chunks_list.push(ch);
                    }
                });
            }
            "jsx_element"
            | "jsx_expression"
            | "jsx_generator_expression"
            | "jsx_text"
            | "jsx_fragment" => {
                if let Some(ch) = compile_child_to_chunks(tree, child, emitter, settings) {
                    child_chunks_list.push(ch);
                }
            }
            "/>" => {
                if child.is_missing() || child.start_byte() == child.end_byte() {
                    emitter.error(
                        format!("Unclosed or malformed opening tag '<{}'", tag_name),
                        tag_name_node.start_byte() - 1,
                        tag_name_node.end_byte(),
                    );
                }
                is_self_closing = true;
            }
            _ => {}
        }
    }

    // Close-tag mismatch check.
    if !is_self_closing {
        let cc = children.len();
        if cc >= 3 {
            let a = children[cc - 3];
            let b = children[cc - 2];
            let c = children[cc - 1];
            let a_text = str::from_utf8(&source[a.start_byte()..a.end_byte()]).unwrap_or("");
            let c_text = str::from_utf8(&source[c.start_byte()..c.end_byte()]).unwrap_or("");
            let is_closing_slash = a.kind() == "</" || a_text == "</";
            let is_closing_name = b.kind() == "identifier" || b.kind() == "jsx_tag_name";
            let is_closing_angle = c.kind() == ">" || c_text == ">";
            if is_closing_slash && is_closing_name && is_closing_angle {
                let end_tag =
                    str::from_utf8(&source[b.start_byte()..b.end_byte()]).unwrap_or("");
                if end_tag != tag_name {
                    emitter.error(
                        format!("Expected tag name '{}', got '{}'", tag_name, end_tag),
                        b.start_byte(),
                        b.end_byte(),
                    );
                }
            }
        }
    }

    if is_component(&tag_name) {
        build_component_chunks(&tag_name, &attrs_kwargs, child_chunks_list, emitter)
    } else {
        build_html_element_chunks(
            &tag_name,
            &attr_kinds,
            child_chunks_list,
            is_self_closing,
            settings,
            emitter,
            (tag_name_node.start_byte(), tag_name_node.end_byte()),
            (node.start_byte(), node.end_byte()),
        )
    }
}

/// Compile one child of a JSX element to a `Chunks` sequence, or `None` if
/// the child contributes no output (e.g. whitespace-only jsx_text).
fn compile_child_to_chunks(
    tree: &Tree,
    child: Node,
    emitter: &mut Emitter,
    settings: &CompilerSettings,
) -> Option<Chunks> {
    let source = emitter.source;
    match child.kind() {
        "jsx_element" | "jsx_fragment" => {
            let chunks = compile_jsx_to_chunks(tree, child, emitter, settings).finish();
            if chunks.is_empty() {
                None
            } else {
                let mut out = Chunks::new();
                for c in chunks {
                    match c {
                        Chunk::Static(s) => out.push_static(s),
                        Chunk::Dynamic(e) => out.push_dynamic(e),
                    }
                }
                Some(out)
            }
        }
        "jsx_text" => {
            let raw =
                str::from_utf8(&source[child.start_byte()..child.end_byte()]).unwrap_or("");
            warn_on_unknown_entities(emitter, raw, child.start_byte());
            let collapsed = collapse_jsx_whitespace(raw)?;
            let decoded = decode_html_entities(&collapsed);
            let escaped = html_escape_text(&decoded);
            if escaped.is_empty() {
                None
            } else {
                let mut c = Chunks::new();
                c.push_static(escaped);
                Some(c)
            }
        }
        "jsx_expression" => {
            // `{*iterable}` routes through OP_UNPACK_ARGS so the iterable
            // is iterated at render time; emitting `_pjr_tpl_N(*iterable)`
            // would Python-unpack at call time and mismatch the slot count.
            // Detect via leading `*` — AST detection is brittle here.
            let src_text = visit_to_string(tree, child, source, settings, emitter);
            let trimmed = src_text.trim_start();
            let is_splat = trimmed.starts_with('*') && !trimmed.starts_with("**");
            if src_text.is_empty() {
                None
            } else if is_splat {
                let expr = trimmed[1..].trim_start();
                let mut c = Chunks::new();
                c.push_dynamic(OP_UNPACK_ARGS.to_string());
                c.push_dynamic(expr.to_string());
                Some(c)
            } else {
                let mut c = Chunks::new();
                c.push_dynamic(OP_ESCAPE_TEXT.to_string());
                c.push_dynamic(src_text);
                Some(c)
            }
        }
        "jsx_generator_expression" => {
            // JSX-body generator (`{<li/> for ...}`) yields JSXResults that
            // INSTR_VALUE iterates directly.  Non-JSX-body generator wraps in
            // OP_UNPACK_ARGS so pjr_render_value's iterable path escapes each.
            let body = child.child_by_field_name("body");
            let body_is_jsx = match body.map(|b| b.kind()) {
                Some("jsx_element") | Some("jsx_fragment") => true,
                _ => false,
            };
            if body_is_jsx {
                let src_text = visit_to_string(tree, child, source, settings, emitter);
                if src_text.is_empty() {
                    None
                } else {
                    let mut c = Chunks::new();
                    c.push_dynamic(src_text);
                    Some(c)
                }
            } else {
                let src_text = visit_to_string(tree, child, source, settings, emitter);
                if src_text.is_empty() {
                    None
                } else {
                    let mut c = Chunks::new();
                    c.push_dynamic(OP_UNPACK_ARGS.to_string());
                    c.push_dynamic(src_text);
                    Some(c)
                }
            }
        }
        _ => None,
    }
}

fn build_html_element_chunks(
    tag_name: &str,
    attr_kinds: &[AttrKind],
    child_chunks_list: Vec<Chunks>,
    is_self_closing: bool,
    settings: &CompilerSettings,
    emitter: &mut Emitter,
    tag_name_range: (usize, usize),
    element_range: (usize, usize),
) -> Chunks {
    // HTML5 void-element handling: non-void self-close `<div/>` expands to
    // `<div></div>` (HTML5 ignores the `/` and would leave it unclosed);
    // void element with content/end-tag is a compile error.
    let is_html_void = settings.is_void_tag(tag_name);
    let has_children = !child_chunks_list.is_empty();
    if is_html_void && (has_children || !is_self_closing) {
        let msg = if has_children {
            format!(
                "void element '<{}>' cannot have children (HTML5 void elements are self-closing)",
                tag_name
            )
        } else {
            format!(
                "void element '<{}>' cannot have an end tag; write '<{}>' or '<{}/>' instead",
                tag_name, tag_name, tag_name
            )
        };
        emitter.error(msg, element_range.0, element_range.1);
        let _ = tag_name_range; // reserved for finer-grained error spans
        let mut chunks = Chunks::new();
        chunks.push_static(format!("<{}/>", tag_name));
        return chunks;
    }

    let mut chunks = Chunks::new();

    let mut static_buf = format!("<{}", tag_name);
    for ak in attr_kinds {
        match ak {
            AttrKind::StaticKV { name, value } => {
                static_buf.push_str(&format!(" {}=\"{}\"", name, html_escape_attr(value)));
            }
            AttrKind::DynamicKV { name, expr } => {
                // RENDER_ATTR preserves None/False→omit and True→bare-name.
                chunks.push_static(std::mem::take(&mut static_buf));
                chunks.push_dynamic(OP_RENDER_ATTR.to_string());
                chunks.push_dynamic(format!("{:?}", name));
                chunks.push_dynamic(expr.clone());
            }
            AttrKind::Boolean { name } => {
                static_buf.push(' ');
                static_buf.push_str(name);
            }
            AttrKind::Spread { expr } => {
                // UNPACK_ATTRS opcode + dict (or mapping) expression.
                chunks.push_static(std::mem::take(&mut static_buf));
                chunks.push_dynamic(OP_UNPACK_ATTRS.to_string());
                chunks.push_dynamic(expr.clone());
            }
        }
    }

    // Void tags close with `/>`; non-void fall through to children + `</tag>`
    // (so `<div/>` → `<div></div>`).
    if is_html_void {
        debug_assert!(is_self_closing && !has_children);
        static_buf.push_str("/>");
        chunks.push_static(static_buf);
        return chunks;
    }

    static_buf.push('>');
    chunks.push_static(static_buf);

    for child in child_chunks_list {
        chunks.extend(child);
    }

    chunks.push_static(format!("</{}>", tag_name));
    chunks
}

fn build_component_chunks(
    tag_name: &str,
    attrs_kwargs: &[String],
    child_chunks_list: Vec<Chunks>,
    emitter: &mut Emitter,
) -> Chunks {
    // Lower each child to the cheapest Python expression the component's
    // SLOT_VALUE path can render: bare `expr` for JSX-body / OP_ESCAPE_TEXT,
    // `*expr` for OP_UNPACK_ARGS, otherwise an eager sub-template.
    let op_escape_text = OP_ESCAPE_TEXT.to_string();
    let op_unpack_args = OP_UNPACK_ARGS.to_string();
    let mut args_py: Vec<String> = Vec::with_capacity(child_chunks_list.len() + attrs_kwargs.len());
    for ch in child_chunks_list {
        let chunks = ch.finish();
        let py = match chunks.as_slice() {
            [] => continue,
            [Chunk::Dynamic(e)] => e.clone(),
            [Chunk::Dynamic(op), Chunk::Dynamic(expr)] if op == &op_escape_text => expr.clone(),
            [Chunk::Dynamic(op), Chunk::Dynamic(expr)] if op == &op_unpack_args => {
                format!("*{}", expr)
            }
            _ => {
                let (template_args, call_args, usage) = as_template_call(&chunks);
                emitter.uses_slot_value |= usage.slot_value;
                emitter.uses_slot_spread |= usage.slot_spread;
                emitter.uses_slot_attr |= usage.slot_attr;
                let name = emitter.next_template_name();
                let eager = call_args.is_empty();
                emitter.emit_template_def(&name, &template_args, eager);
                if eager {
                    name
                } else {
                    format!("{}({})", name, call_args)
                }
            }
        };
        args_py.push(py);
    }
    for a in attrs_kwargs {
        args_py.push(a.clone());
    }
    let call_args = args_py.join(", ");

    let mut chunks = Chunks::new();
    chunks.push_dynamic(format!("{}({})", tag_name, call_args));
    chunks
}

fn compile_jsx_fragment_to_chunks(
    tree: &Tree,
    node: Node,
    emitter: &mut Emitter,
    settings: &CompilerSettings,
) -> Chunks {
    let source = emitter.source;
    let mut chunks = Chunks::new();
    for_each_child(node, |child| {
        let k = child.kind();
        if k == "ERROR" {
            let err_text =
                str::from_utf8(&source[child.start_byte()..child.end_byte()]).unwrap_or("");
            if let Some((msg, placeholder)) = classify_error_node(err_text) {
                emitter.error(msg, child.start_byte(), child.end_byte());
                chunks.push_dynamic(placeholder.to_string());
            }
            return;
        }
        // Route through compile_child_to_chunks so `{expr}` becomes
        // OP_ESCAPE_TEXT (without it, `<>{x}</>` would emit `x` unescaped — XSS).
        if let Some(child_chunks) = compile_child_to_chunks(tree, child, emitter, settings) {
            chunks.extend(child_chunks);
        }
    });
    chunks
}

/// Lower a top-level chunk sequence to its call-site Python expression.
/// Hoists a `_pjr_tpl_N = _pjr_Tpl(...)` constant to module scope and
/// returns `_pjr_tpl_N(args)`.  A single bare Dynamic chunk (already a
/// renderable value) short-circuits — no template needed.
fn serialize_chunks_top_level(chunks: &[Chunk], emitter: &mut Emitter) -> String {
    if chunks.len() == 1 {
        if let Chunk::Dynamic(e) = &chunks[0] {
            return e.clone();
        }
    }
    let (template_args, call_args, usage) = as_template_call(chunks);
    emitter.uses_slot_value |= usage.slot_value;
    emitter.uses_slot_spread |= usage.slot_spread;
    emitter.uses_slot_attr |= usage.slot_attr;
    let name = emitter.next_template_name();
    let eager = call_args.is_empty();
    emitter.emit_template_def(&name, &template_args, eager);
    if eager {
        name
    } else {
        format!("{}({})", name, call_args)
    }
}

fn visit_jsx_element(
    tree: &Tree,
    node: Node,
    emitter: &mut Emitter,
    settings: &CompilerSettings,
) {
    let chunks = compile_jsx_to_chunks(tree, node, emitter, settings).finish();
    let py = serialize_chunks_top_level(&chunks, emitter);
    emitter.emit_generated(node.start_byte(), node.end_byte(), &py);
}

fn visit_jsx_fragment(
    tree: &Tree,
    node: Node,
    emitter: &mut Emitter,
    settings: &CompilerSettings,
) {
    let chunks = compile_jsx_to_chunks(tree, node, emitter, settings).finish();
    let py = serialize_chunks_top_level(&chunks, emitter);
    emitter.emit_generated(node.start_byte(), node.end_byte(), &py);
}

fn visit_jsx_attribute_to_string(
    tree: &Tree,
    node: Node,
    source: &[u8],
    settings: &CompilerSettings,
    emitter: &mut Emitter,
) -> (String, String, Option<String>) {
    let mut attr_name = String::new();
    let mut attr_value = "True".to_string();
    let mut raw_decoded: Option<String> = None;
    let mut cursor = node.walk();
    if cursor.goto_first_child() { loop {
        let child = cursor.node();
        match child.kind() {
            "jsx_attribute_name" => {
                attr_name = str::from_utf8(&source[child.start_byte()..child.end_byte()])
                    .unwrap_or("")
                    .to_string();
            }
            "string" => {
                let raw = str::from_utf8(&source[child.start_byte()..child.end_byte()])
                    .unwrap_or("\"\"");
                let inner = if raw.len() >= 2 {
                    &raw[1..raw.len() - 1]
                } else {
                    ""
                };
                warn_on_unknown_entities(emitter, inner, child.start_byte() + 1);
                let decoded = decode_html_entities(inner);
                attr_value = escape_string(&decoded);
                raw_decoded = Some(decoded);
            }
            "jsx_expression" => {
                attr_value = visit_to_string(tree, child, source, settings, emitter);
                raw_decoded = None;
            }
            "ERROR" => {
                let (s, e) =
                    trim_ws_range(source, child.start_byte(), child.end_byte());
                emitter.error(
                    "Attribute value must be a string or expression: use name=\"value\" or name={expr}",
                    s,
                    e,
                );
            }
            _ => {}
        }
        if !cursor.goto_next_sibling() { break; }
    } }
    (attr_name, attr_value, raw_decoded)
}

fn visit_jsx_spread_attribute_to_string(
    tree: &Tree,
    node: Node,
    emitter: &mut Emitter,
    source: &[u8],
    settings: &CompilerSettings,
) -> String {
    // Grammar: `{` `**` expression `}` (4 children).  On failure, dispatch on
    // ERROR contents for a specific message.
    if node.child_count() != 4 {
        // `{**a, **b}` (multiple spreads) is explicitly disallowed.
        let mut saw_comma_in_error = false;
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let c = cursor.node();
                if c.kind() == "ERROR" {
                    let txt = str::from_utf8(&source[c.start_byte()..c.end_byte()]).unwrap_or("");
                    if txt.contains(',') {
                        saw_comma_in_error = true;
                        break;
                    }
                }
                if !cursor.goto_next_sibling() { break; }
            }
        }
        let msg = if saw_comma_in_error {
            "Spread attribute must be a single {**expr}; write multiple spreads as separate attributes"
        } else {
            "Invalid spread attribute: expected {**expr}"
        };
        emitter.error(msg, node.start_byte(), node.end_byte());
        return "**{}".to_string();
    }
    let expr_node = node.child(2).unwrap();
    let expr = visit_to_string(tree, expr_node, source, settings, emitter);
    format!("**{}", expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_python_kwarg_accepts_ascii_identifier() {
        assert!(valid_python_kwarg("title"));
        assert!(valid_python_kwarg("data_value"));
        assert!(valid_python_kwarg("x1"));
        assert!(valid_python_kwarg("_private"));
    }

    #[test]
    fn valid_python_kwarg_rejects_hard_keywords() {
        assert!(!valid_python_kwarg("class"));
        assert!(!valid_python_kwarg("for"));
        assert!(!valid_python_kwarg("return"));
    }

    #[test]
    fn valid_python_kwarg_rejects_dashes() {
        assert!(!valid_python_kwarg("data-id"));
        assert!(!valid_python_kwarg("aria-label"));
    }

    #[test]
    fn valid_python_kwarg_rejects_non_ascii_digits_that_look_alphanumeric() {
        // Other_Number (²³½) is is_alphanumeric() but not XID_Continue —
        // the Python tokenizer would reject `foo²=...` as SyntaxError.
        assert!(!valid_python_kwarg("foo\u{00B2}"));
        assert!(!valid_python_kwarg("foo\u{00BD}"));
        assert!(!valid_python_kwarg("\u{00B2}"));
    }

    #[test]
    fn valid_python_kwarg_rejects_leading_digit() {
        assert!(!valid_python_kwarg("1foo"));
    }
}
