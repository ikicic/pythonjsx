//! JSX-only formatter for PythonJSX source.

use crate::compiler::html_entities::collapse_jsx_whitespace;
use crate::compiler::{compile, CompileErrorSeverity, CompilerSettings};
use crate::parser;
use std::fmt;
use tree_sitter::{Node, Tree};

#[derive(Debug, Clone)]
pub struct FormatSettings {
    pub line_width: usize,
    pub indent_width: usize,
    pub preserve_multiline: bool,
}

impl Default for FormatSettings {
    fn default() -> Self {
        Self {
            line_width: 100,
            indent_width: 4,
            preserve_multiline: true,
        }
    }
}

#[derive(Debug)]
pub enum FormatError {
    Parse(tree_sitter::LanguageError),
    Syntax,
    InvalidSource(Vec<String>),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "parse error: {}", e),
            Self::Syntax => write!(f, "source contains syntax errors"),
            Self::InvalidSource(messages) => {
                if messages.is_empty() {
                    write!(f, "source contains invalid PythonJSX")
                } else {
                    write!(f, "source contains invalid PythonJSX: {}", messages.join("; "))
                }
            }
        }
    }
}

impl std::error::Error for FormatError {}

pub fn format_source(source: &str, settings: &FormatSettings) -> Result<String, FormatError> {
    let tree = parser::parse(source).map_err(FormatError::Parse)?;
    if parser::has_parse_errors(&tree) {
        return Err(FormatError::Syntax);
    }

    let (_compiled, _source_map, errors) =
        compile(source, &CompilerSettings::default()).map_err(FormatError::Parse)?;
    let errors = errors
        .into_iter()
        .filter(|e| e.severity == CompileErrorSeverity::Error)
        .map(|e| e.message)
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(FormatError::InvalidSource(errors));
    }

    let roots = jsx_roots(&tree);
    if roots.is_empty() {
        return Ok(source.to_string());
    }

    let mut out = String::with_capacity(source.len());
    let mut pos = 0;
    for node in roots {
        out.push_str(&source[pos..node.start_byte()]);
        out.push_str(&format_jsx_root(node, source, settings));
        pos = node.end_byte();
    }
    out.push_str(&source[pos..]);
    Ok(out)
}

fn jsx_roots(tree: &Tree) -> Vec<Node<'_>> {
    let mut roots = Vec::new();
    collect_jsx_roots(tree.root_node(), &mut roots);
    roots
}

fn collect_jsx_roots<'a>(node: Node<'a>, roots: &mut Vec<Node<'a>>) {
    if is_jsx_node(node) {
        roots.push(node);
        return;
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_jsx_roots(cursor.node(), roots);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn is_jsx_node(node: Node) -> bool {
    matches!(node.kind(), "jsx_element" | "jsx_fragment")
}

fn jsx_start_column(source: &str, start: usize) -> String {
    let line_start = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = source[line_start..start].chars().count();
    " ".repeat(col)
}

fn format_jsx_root(node: Node, source: &str, settings: &FormatSettings) -> String {
    let indent = jsx_start_column(source, node.start_byte());
    let formatted = format_jsx_node(node, source, settings, &indent);
    let Some(parent) = node.parent() else {
        return formatted;
    };
    if !needs_python_expression_guard(parent) {
        return formatted;
    }

    let line_start = source[..node.start_byte()]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let prefix = &source[line_start..node.start_byte()];
    let stmt_indent_len = prefix
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .count();
    let stmt_indent = " ".repeat(stmt_indent_len);
    let jsx_indent = format!("{}{}", stmt_indent, " ".repeat(settings.indent_width));
    let line_width = prefix.chars().count() + formatted.chars().count();

    if !formatted.contains('\n') && line_width <= settings.line_width {
        return formatted;
    }

    let inner = format_jsx_node(node, source, settings, &jsx_indent);
    format!("(\n{}{}\n{})", jsx_indent, inner, stmt_indent)
}

fn needs_python_expression_guard(parent: Node) -> bool {
    matches!(
        parent.kind(),
        "assignment" | "return_statement" | "expression_statement"
    )
}

fn child_nodes<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::with_capacity(node.child_count());
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

fn source_of<'a>(source: &'a str, node: Node) -> &'a str {
    &source[node.start_byte()..node.end_byte()]
}

fn format_jsx_node(
    node: Node,
    source: &str,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    match node.kind() {
        "jsx_element" => format_element(node, source, settings, indent),
        "jsx_fragment" => format_fragment(node, source, settings, indent),
        _ => source_of(source, node).to_string(),
    }
}

#[derive(Clone)]
struct ElementParts<'a> {
    tag: String,
    attrs: Vec<String>,
    children: Vec<Child<'a>>,
    self_closing: bool,
}

#[derive(Clone)]
enum Child<'a> {
    Text(String),
    Expr(Node<'a>),
    Element(Node<'a>),
    Fragment(Node<'a>),
}

fn format_element(
    node: Node,
    source: &str,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    let parts = parse_element(node, source, settings);
    if parts.self_closing {
        let inline = inline_opening(&parts, true);
        if fits(indent, &inline, settings) && !attrs_need_block(&parts.attrs) {
            inline
        } else {
            block_opening(&parts, true, settings, indent)
        }
    } else {
        format_container(
            &parts.tag,
            &parts.attrs,
            &parts.children,
            source,
            settings,
            indent,
            ContainerKind::Element,
            settings.preserve_multiline && source_of(source, node).contains('\n'),
        )
    }
}

fn parse_element<'a>(
    node: Node<'a>,
    source: &str,
    settings: &FormatSettings,
) -> ElementParts<'a> {
    let children = child_nodes(node);
    let tag_node = children
        .iter()
        .copied()
        .find(|c| is_name_node(*c))
        .expect("valid JSX element has a tag name");
    let tag = source_of(source, tag_node).to_string();
    let attrs = children
        .iter()
        .copied()
        .filter(|c| is_attr_node(*c))
        .map(|c| format_attr(c, source, settings))
        .collect::<Vec<_>>();
    let self_closing = children.iter().any(|c| c.kind() == "/>");
    let body = children
        .iter()
        .copied()
        .filter_map(|c| child_from_node(c, source))
        .collect::<Vec<_>>();

    ElementParts {
        tag,
        attrs,
        children: body,
        self_closing,
    }
}

fn child_from_node<'a>(node: Node<'a>, source: &str) -> Option<Child<'a>> {
    match node.kind() {
        "jsx_text" => collapse_jsx_whitespace(source_of(source, node)).map(Child::Text),
        "jsx_expression" | "jsx_generator_expression" => {
            Some(Child::Expr(node))
        }
        "jsx_element" => Some(Child::Element(node)),
        "jsx_fragment" => Some(Child::Fragment(node)),
        _ => None,
    }
}

fn is_name_node(node: Node) -> bool {
    matches!(node.kind(), "identifier" | "jsx_tag_name" | "_jsx_element_name")
}

fn is_attr_node(node: Node) -> bool {
    matches!(
        node.kind(),
        "jsx_attribute" | "jsx_boolean_attribute" | "jsx_spread_attribute"
    )
}

fn format_attr(node: Node, source: &str, settings: &FormatSettings) -> String {
    if node.kind() != "jsx_attribute" {
        return normalize_multiline_attr(source_of(source, node).trim());
    }

    let children = child_nodes(node);
    let Some(name_node) = children.iter().copied().find(|c| c.kind() == "jsx_attribute_name")
    else {
        return normalize_multiline_attr(source_of(source, node).trim());
    };
    let Some(expr_node) = children.iter().copied().find(|c| c.kind() == "jsx_expression") else {
        return normalize_multiline_attr(source_of(source, node).trim());
    };
    let roots = jsx_roots_inside(expr_node);
    if roots.is_empty() {
        return normalize_multiline_attr(source_of(source, node).trim());
    }

    let name = source_of(source, name_node);
    let expr = format_attr_jsx_expression(expr_node, source, settings);
    format!("{}={}", name, expr)
}

fn normalize_multiline_attr(attr: &str) -> String {
    if !attr.contains('\n') {
        return attr.to_string();
    }

    let lines: Vec<&str> = attr.lines().collect();
    let common_indent = lines
        .iter()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);

    let mut out = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if idx == 0 {
            out.push_str(line.trim());
        } else if line.len() >= common_indent {
            out.push_str(&line[common_indent..]);
        } else {
            out.push_str(line.trim_start());
        }
    }
    out
}

fn format_attr_jsx_expression(
    node: Node,
    source: &str,
    settings: &FormatSettings,
) -> String {
    let roots = jsx_roots_inside(node);
    if roots.len() != 1 {
        return format_jsx_expression(node, source, settings, "    ");
    }

    let root = roots[0];
    let before = source[node.start_byte() + 1..root.start_byte()].trim();
    let after = source[root.end_byte()..node.end_byte() - 1].trim();
    if !before.is_empty() || !after.is_empty() {
        return format_jsx_expression(node, source, settings, "    ");
    }

    let formatted = format_jsx_node(root, source, settings, "    ");
    if formatted.contains('\n') {
        format!("{{\n    {}\n}}", formatted)
    } else {
        format!("{{{}}}", formatted)
    }
}

#[derive(Copy, Clone)]
enum ContainerKind {
    Element,
    Fragment,
}

fn format_container(
    tag: &str,
    attrs: &[String],
    children: &[Child],
    source: &str,
    settings: &FormatSettings,
    indent: &str,
    kind: ContainerKind,
    preserve_block: bool,
) -> String {
    let inline_open = match kind {
        ContainerKind::Element => inline_opening_parts(tag, attrs, false),
        ContainerKind::Fragment => "<>".to_string(),
    };
    let inline_close = match kind {
        ContainerKind::Element => format!("</{}>", tag),
        ContainerKind::Fragment => "</>".to_string(),
    };
    let inline_children = children_inline(children, source, settings, indent);
    let inline = format!("{}{}{}", inline_open, inline_children, inline_close);

    if should_force_inline(children) {
        return inline;
    }

    if !preserve_block
        && can_inline(children)
        && fits(indent, &inline, settings)
        && !attrs_need_block(attrs)
    {
        return inline;
    }

    if !preserve_block && children.len() == 1 {
        if let Child::Text(t) = &children[0] {
            if t.starts_with(char::is_whitespace) || t.ends_with(char::is_whitespace) {
                return inline;
            }
        }
    }

    let mut out = String::new();
    let opening = if attrs_need_block(attrs) || !fits(indent, &inline_open, settings) {
        block_opening_parts(tag, attrs, false, settings, indent)
    } else {
        inline_open
    };
    let child_indent = format!("{}{}", indent, " ".repeat(settings.indent_width));
    out.push_str(&opening);
    for child in children {
        out.push('\n');
        out.push_str(&child_indent);
        out.push_str(&format_child_block(child, source, settings, &child_indent));
    }
    out.push('\n');
    out.push_str(indent);
    out.push_str(&inline_close);
    out
}

fn format_fragment(
    node: Node,
    source: &str,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    let children = child_nodes(node)
        .into_iter()
        .filter_map(|c| child_from_node(c, source))
        .collect::<Vec<_>>();
    format_container(
        "",
        &[],
        &children,
        source,
        settings,
        indent,
        ContainerKind::Fragment,
        settings.preserve_multiline && source_of(source, node).contains('\n'),
    )
}

fn inline_opening(parts: &ElementParts, self_closing: bool) -> String {
    inline_opening_parts(&parts.tag, &parts.attrs, self_closing)
}

fn inline_opening_parts(tag: &str, attrs: &[String], self_closing: bool) -> String {
    let mut out = format!("<{}", tag);
    for attr in attrs {
        out.push(' ');
        out.push_str(attr);
    }
    if self_closing {
        out.push_str(" />");
    } else {
        out.push('>');
    }
    out
}

fn block_opening(
    parts: &ElementParts,
    self_closing: bool,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    block_opening_parts(&parts.tag, &parts.attrs, self_closing, settings, indent)
}

fn block_opening_parts(
    tag: &str,
    attrs: &[String],
    self_closing: bool,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    let attr_indent = format!("{}{}", indent, " ".repeat(settings.indent_width));
    let mut out = format!("<{}", tag);
    for attr in attrs {
        out.push('\n');
        append_indented_multiline(&mut out, attr, &attr_indent);
    }
    out.push('\n');
    out.push_str(indent);
    if self_closing {
        out.push_str("/>");
    } else {
        out.push('>');
    }
    out
}

fn append_indented_multiline(out: &mut String, text: &str, prefix: &str) {
    for (idx, line) in text.lines().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(prefix);
        out.push_str(line);
    }
}

fn attrs_need_block(attrs: &[String]) -> bool {
    attrs.iter().any(|a| a.contains('\n'))
}

fn children_inline(
    children: &[Child],
    source: &str,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    let mut out = String::new();
    for child in children {
        out.push_str(&format_child_inline(child, source, settings, indent));
    }
    out
}

fn format_child_inline(
    child: &Child,
    source: &str,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    match child {
        Child::Text(s) => s.clone(),
        Child::Expr(n) => format_jsx_expression(*n, source, settings, ""),
        Child::Element(n) | Child::Fragment(n) => {
            let inline_settings = FormatSettings {
                preserve_multiline: false,
                ..settings.clone()
            };
            format_jsx_node(*n, source, &inline_settings, indent)
                .trim()
                .to_string()
        }
    }
}

fn format_child_block(
    child: &Child,
    source: &str,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    match child {
        Child::Text(s) => s.trim().to_string(),
        Child::Expr(n) => format_jsx_expression(*n, source, settings, indent),
        Child::Element(n) | Child::Fragment(n) => format_jsx_node(*n, source, settings, indent),
    }
}

fn format_jsx_expression(
    node: Node,
    source: &str,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    let roots = jsx_roots_inside(node);
    if roots.is_empty() {
        return source_of(source, node).to_string();
    }

    let mut out = String::with_capacity(node.end_byte() - node.start_byte());
    let mut pos = node.start_byte();
    for root in roots {
        let (start, end, formatted) = format_expression_jsx_unit(root, node, source, settings, indent);
        if start < pos {
            continue;
        }
        out.push_str(&source[pos..start]);
        out.push_str(&formatted);
        pos = end;
    }
    out.push_str(&source[pos..node.end_byte()]);
    out
}

fn jsx_roots_inside<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut roots = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_jsx_roots(cursor.node(), &mut roots);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    roots
}

fn format_nested_jsx_in_expression(
    node: Node,
    source: &str,
    settings: &FormatSettings,
    indent: &str,
) -> String {
    let expr_indent = if indent.is_empty() {
        jsx_start_column(source, node.start_byte())
    } else {
        indent.to_string()
    };
    let inner_indent = format!("{}{}", expr_indent, " ".repeat(settings.indent_width));
    let formatted = format_jsx_node(node, source, settings, &inner_indent);
    if formatted.contains('\n') {
        if node
            .parent()
            .map(|p| p.kind() == "parenthesized_expression")
            .unwrap_or(false)
        {
            return formatted;
        }
        format!("(\n{}{}\n{})", inner_indent, formatted, expr_indent)
    } else {
        formatted
    }
}

fn format_expression_jsx_unit(
    root: Node,
    expression: Node,
    source: &str,
    settings: &FormatSettings,
    indent: &str,
) -> (usize, usize, String) {
    if let Some(parenthesized) = simple_parenthesized_jsx_parent(root, expression, source) {
        let expr_indent = if indent.is_empty() {
            jsx_start_column(source, parenthesized.start_byte())
        } else {
            indent.to_string()
        };
        let inner_indent = format!("{}{}", expr_indent, " ".repeat(settings.indent_width));
        let formatted = format_jsx_node(root, source, settings, &inner_indent);
        return (
            parenthesized.start_byte(),
            parenthesized.end_byte(),
            format!("(\n{}{}\n{})", inner_indent, formatted, expr_indent),
        );
    }

    (
        root.start_byte(),
        root.end_byte(),
        format_nested_jsx_in_expression(root, source, settings, indent),
    )
}

fn simple_parenthesized_jsx_parent<'a>(
    root: Node<'a>,
    expression: Node<'a>,
    source: &str,
) -> Option<Node<'a>> {
    let parent = root.parent()?;
    if parent.kind() != "parenthesized_expression"
        || parent.start_byte() < expression.start_byte()
        || parent.end_byte() > expression.end_byte()
    {
        return None;
    }

    let before = source[parent.start_byte() + 1..root.start_byte()].trim();
    let after = source[root.end_byte()..parent.end_byte() - 1].trim();
    if before.is_empty() && after.is_empty() {
        Some(parent)
    } else {
        None
    }
}

fn should_force_inline(children: &[Child]) -> bool {
    if children.len() <= 1 {
        return false;
    }
    let has_text = children.iter().any(|c| matches!(c, Child::Text(_)));
    let has_non_text = children.iter().any(|c| !matches!(c, Child::Text(_)));
    has_text
        && has_non_text
        && children.iter().any(|c| {
            matches!(
                c,
                Child::Text(text)
                    if text.starts_with(char::is_whitespace)
                        || text.ends_with(char::is_whitespace)
            )
        })
}

fn can_inline(children: &[Child]) -> bool {
    if matches!(
        children,
        [Child::Expr(node)] if node.kind() == "jsx_generator_expression"
    ) {
        return false;
    }
    if children.len() <= 1 {
        return true;
    }
    children.iter().all(|c| matches!(c, Child::Text(_)))
}

fn fits(indent: &str, text: &str, settings: &FormatSettings) -> bool {
    !text.contains('\n') && indent.chars().count() + text.chars().count() <= settings.line_width
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str, width: usize) -> String {
        fmt_with_settings(
            src,
            &FormatSettings {
                line_width: width,
                ..FormatSettings::default()
            },
        )
    }

    fn fmt_with_settings(src: &str, settings: &FormatSettings) -> String {
        let once = format_source(src, settings).unwrap();
        let twice = format_source(&once, settings).unwrap();
        assert_eq!(once, twice, "formatter output is not idempotent");
        assert_generated_templates_equal(src, &once);
        once
    }

    fn fmt_collapse_multiline(src: &str, width: usize) -> String {
        fmt_with_settings(
            src,
            &FormatSettings {
                line_width: width,
                preserve_multiline: false,
                ..FormatSettings::default()
            },
        )
    }

    fn assert_generated_templates_equal(original: &str, formatted: &str) {
        let settings = crate::compiler::CompilerSettings::default();
        let (original_py, _, original_errors) =
            crate::compiler::compile(original, &settings).unwrap();
        let (formatted_py, _, formatted_errors) =
            crate::compiler::compile(formatted, &settings).unwrap();

        assert!(original_errors.is_empty(), "{:?}", original_errors);
        assert!(formatted_errors.is_empty(), "{:?}", formatted_errors);

        let original_templates = generated_template_lines(&original_py);
        let formatted_templates = generated_template_lines(&formatted_py);
        assert_eq!(
            original_templates, formatted_templates,
            "formatter changed generated template objects\noriginal source:\n{}\nformatted source:\n{}\noriginal compiled:\n{}\nformatted compiled:\n{}",
            original, formatted, original_py, formatted_py
        );
    }

    fn generated_template_lines(compiled: &str) -> Vec<&str> {
        compiled
            .lines()
            .filter(|line| line.starts_with("_pjr_tpl_"))
            .collect()
    }

    #[test]
    fn long_text_element_splits() {
        assert_eq!(
            fmt(
                r#"def f():
    return <a href={something_very_long_goes_here} class="also very long here">And a very long text here</a>
"#,
                80,
            ),
            r#"def f():
    return (
        <a href={something_very_long_goes_here} class="also very long here">
            And a very long text here
        </a>
    )
"#
        );
    }

    #[test]
    fn long_self_closing_tag_splits() {
        assert_eq!(
            fmt(
                r#"x = <Component very_long_attr={very_long_value} asdf="Hello world! bla bla bla" foo={1234} />
"#,
                70,
            ),
            r#"x = (
    <Component
        very_long_attr={very_long_value}
        asdf="Hello world! bla bla bla"
        foo={1234}
    />
)
"#
        );
    }

    #[test]
    fn short_multiline_jsx_stays_block_formatted() {
        assert_eq!(
            fmt(
                r#"x = <a
    href={url}
>
    Home
</a>
"#,
                100,
            ),
            r#"x = (
    <a href={url}>
        Home
    </a>
)
"#
        );
    }

    #[test]
    fn collapse_multiline_mode_collapses_simple_multiline_jsx_when_it_fits() {
        assert_eq!(
            fmt_collapse_multiline(
                r#"x = <a
    href={url}
>
    Home
</a>
"#,
                100,
            ),
            r#"x = <a href={url}>Home</a>
"#
        );
    }

    #[test]
    fn existing_multiline_nested_tag_stays_block_formatted() {
        assert_eq!(
            fmt(
                r#"def f():
    return (
        <div>
            <span>hello</span>
        </div>
    )
"#,
                100,
            ),
            r#"def f():
    return (
        <div>
            <span>hello</span>
        </div>
    )
"#
        );
    }

    #[test]
    fn collapse_multiline_mode_collapses_simple_nested_multiline_jsx() {
        assert_eq!(
            fmt_collapse_multiline(
                r#"def f():
    return (
        <div>
            <span>hello</span>
        </div>
    )
"#,
                100,
            ),
            r#"def f():
    return (
        <div><span>hello</span></div>
    )
"#
        );
    }

    #[test]
    fn nested_structural_children_use_block_layout() {
        assert_eq!(
            fmt(r#"x = <div><Header /><main>{children}</main></div>"#, 100),
            r#"x = (
    <div>
        <Header />
        <main>{children}</main>
    </div>
)"#
        );
    }

    #[test]
    fn fragments_format_as_blocks() {
        assert_eq!(
            fmt(r#"x = <><Header /><Content /></>"#, 100),
            r#"x = (
    <>
        <Header />
        <Content />
    </>
)"#
        );
    }

    #[test]
    fn mixed_phrasing_stays_inline_even_when_long() {
        let src = r#"x = <p>Hello {name}, welcome to <b>{site}</b>!</p>"#;
        assert_eq!(
            fmt(src, 20),
            r#"x = (
    <p>Hello {name}, welcome to <b>{site}</b>!</p>
)"#
        );
    }

    #[test]
    fn mixed_phrasing_with_multiline_nested_child_stays_inline() {
        assert_eq!(
            fmt(
                r#"x = <p>Hello <b>
    world
</b>!</p>
"#,
                100,
            ),
            r#"x = <p>Hello <b>world</b>!</p>
"#
        );
    }

    #[test]
    fn collapse_multiline_mixed_phrasing_with_multiline_nested_child_stays_inline() {
        assert_eq!(
            fmt_collapse_multiline(
                r#"x = <p>Hello <b>
    world
</b>!</p>
"#,
                20,
            ),
            r#"x = (
    <p>Hello <b>world</b>!</p>
)
"#
        );
    }

    #[test]
    fn mixed_phrasing_with_long_component_child_does_not_gain_leading_text_space() {
        assert_eq!(
            fmt(
                r#"x = <p>Hello <Component really_long_attribute={some_really_long_value} another_really_long_attribute={another_really_long_value}>
    text
</Component> world</p>
"#,
                40,
            ),
            r#"x = (
    <p>Hello <Component
        really_long_attribute={some_really_long_value}
        another_really_long_attribute={another_really_long_value}
    >
        text
    </Component> world</p>
)
"#
        );
    }

    #[test]
    fn label_with_input_and_text_child_stays_block_formatted() {
        assert_eq!(
            fmt(
                r#"def f():
    return (
        <fieldset class="content-fieldset">
            {language_count > 1 and (
                <label class="checkbox-label content-original-language">
                    <input
                        type="radio"
                        name="original_language"
                        value={lang}
                        checked={is_original}
                    />
                    Original language
                </label>
            )}
        </fieldset>
    )
"#,
                100,
            ),
            r#"def f():
    return (
        <fieldset class="content-fieldset">
            {language_count > 1 and (
                <label class="checkbox-label content-original-language">
                    <input
                        type="radio"
                        name="original_language"
                        value={lang}
                        checked={is_original}
                    />
                    Original language
                </label>
            )}
        </fieldset>
    )
"#
        );
    }

    #[test]
    fn expression_source_is_preserved() {
        let src = r#"x = <div>{  some_call( a,
    b )  }</div>"#;
        let formatted = fmt(src, 120);
        assert_eq!(
            formatted,
            r#"x = (
    <div>
        {  some_call( a,
    b )  }
    </div>
)"#
        );
        assert!(formatted.contains("{  some_call( a,\n    b )  }"));
    }

    #[test]
    fn whitespace_only_text_nodes_are_removed() {
        assert_eq!(
            fmt(
                r#"x = <div>
    <Header />

    <Content />
</div>"#,
                100,
            ),
            r#"x = (
    <div>
        <Header />
        <Content />
    </div>
)"#
        );
    }

    #[test]
    fn formatting_is_idempotent() {
        let once = fmt(r#"x = <div><Header /><Content /></div>"#, 100);
        let twice = fmt(&once, 100);
        assert_eq!(once, twice);
    }

    #[test]
    fn syntax_errors_are_rejected() {
        let result = format_source("x = <div><span></div>", &FormatSettings::default());
        assert!(matches!(result, Err(FormatError::Syntax)));
    }

    #[test]
    fn jsx_compile_errors_are_rejected() {
        let result = format_source("x = <div></span>\n", &FormatSettings::default());
        match result {
            Err(FormatError::InvalidSource(messages)) => {
                assert!(
                    messages.iter().any(|m| m.contains("Expected tag name")),
                    "{:?}",
                    messages
                );
            }
            other => panic!("expected invalid source error, got {:?}", other),
        }
    }

    #[test]
    fn formatted_structural_jsx_compiles_the_same() {
        let original = r#"def App():
    return <div><Header /><main>{children}</main></div>
"#;
        let formatted = fmt(original, 100);
        let settings = crate::compiler::CompilerSettings::default();
        let (original_py, _, original_errors) = crate::compiler::compile(original, &settings).unwrap();
        let (formatted_py, _, formatted_errors) = crate::compiler::compile(&formatted, &settings).unwrap();

        assert!(original_errors.is_empty(), "{:?}", original_errors);
        assert!(formatted_errors.is_empty(), "{:?}", formatted_errors);
        assert!(original_py.contains("_pjr_tpl_0(Header(), children)"));
        assert!(formatted_py.contains("_pjr_tpl_0(Header(), children)"));
    }

    #[test]
    fn short_return_stays_inline() {
        assert_eq!(
            fmt(
                r#"def App():
    return <div>ok</div>
"#,
                100,
            ),
            r#"def App():
    return <div>ok</div>
"#
        );
    }

    #[test]
    fn multiline_return_jsx_is_parenthesized() {
        assert_eq!(
            fmt(
                r#"def App():
    return <div>veeeeeeeeeeeeeeeeeeeery long string</div>
"#,
                30,
            ),
            r#"def App():
    return (
        <div>
            veeeeeeeeeeeeeeeeeeeery long string
        </div>
    )
"#
        );
    }

    #[test]
    fn long_assignment_prefix_parenthesizes_inline_jsx() {
        assert_eq!(
            fmt(
                "very_long_variable_name_that_makes_div_not_fit = <div>bla bla</div>\n",
                40,
            ),
            "very_long_variable_name_that_makes_div_not_fit = (\n    <div>bla bla</div>\n)\n",
        );
    }

    #[test]
    fn jsx_inside_call_arguments_does_not_get_extra_parentheses() {
        assert_eq!(
            fmt("x = foo(<div>veeeeeeeeeeeeeeeeeeeery long string</div>)\n", 30),
            "x = foo(<div>\n            veeeeeeeeeeeeeeeeeeeery long string\n        </div>)\n",
        );
    }

    #[test]
    fn already_parenthesized_jsx_does_not_get_nested_parentheses() {
        assert_eq!(
            fmt(
                r#"x = (
    <div><Header /><Content /></div>
)
"#,
                100,
            ),
            r#"x = (
    <div>
        <Header />
        <Content />
    </div>
)
"#
        );
    }

    #[test]
    fn nested_tags_all_reformat_due_to_width() {
        assert_eq!(
            fmt(
                "x = <Outer very_long_attr={outer_value}><Inner very_long_attr={inner_value}>veeeeeery long text</Inner></Outer>\n",
                30,
            ),
            r#"x = (
    <Outer
        very_long_attr={outer_value}
    >
        <Inner
            very_long_attr={inner_value}
        >
            veeeeeery long text
        </Inner>
    </Outer>
)
"#
        );
    }

    #[test]
    fn nested_self_closing_tags_all_reformat_due_to_width() {
        assert_eq!(
            fmt(
                "x = <Outer very_long_attr={outer_value}><Inner very_long_attr={inner_value} /><Other very_long_attr={other_value} /></Outer>\n",
                34,
            ),
            r#"x = (
    <Outer
        very_long_attr={outer_value}
    >
        <Inner
            very_long_attr={inner_value}
        />
        <Other
            very_long_attr={other_value}
        />
    </Outer>
)
"#
        );
    }

    #[test]
    fn component_with_jsx_attribute_and_child_formats_cleanly() {
        assert_eq!(
            fmt(
                "x = <Page header={<h1>Hello</h1>}><div>Some large content</div></Page>\n",
                40,
            ),
            r#"x = (
    <Page header={<h1>Hello</h1>}>
        <div>Some large content</div>
    </Page>
)
"#
        );
    }

    #[test]
    fn long_jsx_attribute_expression_reformats_nested_jsx() {
        assert_eq!(
            fmt(
                "x = <Page header={<h1>Veeeeeeeeeeeeeeeeeeery long title</h1>}><div>Content</div></Page>\n",
                35,
            ),
            r#"x = (
    <Page
        header={
            <h1>
                Veeeeeeeeeeeeeeeeeeery long title
            </h1>
        }
    >
        <div>Content</div>
    </Page>
)
"#
        );
    }

    #[test]
    fn conditional_jsx_expression_children_stay_readable() {
        assert_eq!(
            fmt(
                "x = <Page>{show_first and <div>First</div>}{show_second and <div>Second</div>}</Page>\n",
                40,
            ),
            r#"x = (
    <Page>
        {show_first and <div>First</div>}
        {show_second and <div>Second</div>}
    </Page>
)
"#
        );
    }

    #[test]
    fn conditional_jsx_expression_child_reformats_long_nested_jsx() {
        assert_eq!(
            fmt(
                "x = <Page>{show_first and <div>veeeeeeeeeeeeeeeeeeeery long string</div>}{show_second and <div>Second</div>}</Page>\n",
                35,
            ),
            r#"x = (
    <Page>
        {show_first and (
            <div>
                veeeeeeeeeeeeeeeeeeeery long string
            </div>
        )}
        {show_second and <div>Second</div>}
    </Page>
)
"#
        );
    }

    #[test]
    fn existing_parenthesized_component_with_conditional_children_stays_block_formatted() {
        assert_eq!(
            fmt(
                r#"def view():
    return (
        <Page item={item}
              title={item.title}
              nav={make_nav(item)}
              actions={make_actions(item)}>
            <div class="panel-controls">
                {items and (
                    <div class="item-list with-secondary-style">
                        {render_item(i) for i in items}
                    </div>
                )}
            </div>
            {content and (
                <div class="content-card with-main-body-style">
                    {render_content(content)}
                </div>
            )}
            {content and render_extra(content)}
            {not content and (
                <p class="muted">{_("No content available.")}</p>
            )}
            {footer_fragment(item)}
        </Page>
    )
"#,
                80,
            ),
            r#"def view():
    return (
        <Page
            item={item}
            title={item.title}
            nav={make_nav(item)}
            actions={make_actions(item)}
        >
            <div class="panel-controls">
                {items and (
                    <div class="item-list with-secondary-style">
                        {render_item(i) for i in items}
                    </div>
                )}
            </div>
            {content and (
                <div class="content-card with-main-body-style">
                    {render_content(content)}
                </div>
            )}
            {content and render_extra(content)}
            {not content and (
                <p class="muted">{_("No content available.")}</p>
            )}
            {footer_fragment(item)}
        </Page>
    )
"#
        );
    }

    #[test]
    fn multiline_attribute_expression_keeps_relative_indentation() {
        assert_eq!(
            fmt(
                r#"def view():
    return (
        <Page
            request={request}
            title={content["help_title"]}
            breadcrumbs={[
                (_("Home"), reverse("home")),
                (content["help_title"], None),
            ]}
        >
            Hello
        </Page>
    )
"#,
                100,
            ),
            r#"def view():
    return (
        <Page
            request={request}
            title={content["help_title"]}
            breadcrumbs={[
                (_("Home"), reverse("home")),
                (content["help_title"], None),
            ]}
        >
            Hello
        </Page>
    )
"#
        );
    }

    #[test]
    fn generator_expression_child_stays_block_formatted() {
        assert_eq!(
            fmt(
                r#"def f():
    return (
        <ul class="form-errors">
            {<li>{e}</li> for e in errors}
        </ul>
    )
"#,
                100,
            ),
            r#"def f():
    return (
        <ul class="form-errors">
            {<li>{e}</li> for e in errors}
        </ul>
    )
"#
        );
    }

    #[test]
    fn collapse_multiline_mode_keeps_generator_expression_child_block_formatted() {
        assert_eq!(
            fmt_collapse_multiline(
                r#"def f():
    return (
        <ul class="form-errors">
            {<li>{e}</li> for e in errors}
        </ul>
    )
"#,
                100,
            ),
            r#"def f():
    return (
        <ul class="form-errors">
            {<li>{e}</li> for e in errors}
        </ul>
    )
"#
        );
    }
}
