//! Tree-sitter parser for Python+JSX (.px) files.

use tree_sitter::{Language, Node, Parser, Tree};

extern "C" {
    fn tree_sitter_pythonjsx() -> *const tree_sitter::ffi::TSLanguage;
}

fn language() -> Language {
    unsafe { Language::from_raw(tree_sitter_pythonjsx()) }
}

/// Parse source code into a syntax tree.
pub fn parse(source: &str) -> Result<Tree, tree_sitter::LanguageError> {
    let mut parser = Parser::new();
    parser.set_language(&language())?;
    parser.parse(source, None).ok_or_else(|| tree_sitter::LanguageError::Version(0))
}

/// Parse and return the tree. Returns None if parsing failed.
pub fn parse_or_null(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&language()).ok()?;
    parser.parse(source, None)
}

/// Check if the tree has parse errors (ERROR nodes).
pub fn has_parse_errors(tree: &Tree) -> bool {
    let root = tree.root_node();
    has_error_node(root, tree)
}

fn has_error_node(node: Node, tree: &Tree) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut cur = node.walk();
    if cur.goto_first_child() {
        loop {
            if has_error_node(cur.node(), tree) {
                return true;
            }
            if !cur.goto_next_sibling() {
                break;
            }
        }
    }
    false
}

/// Get the root node's kind (e.g. "module").
pub fn root_kind(tree: &Tree) -> &'static str {
    tree.root_node().kind()
}

/// Print tree structure for debugging.
pub fn print_tree(tree: &Tree, source: &str) {
    fn print_node(node: tree_sitter::Node, source: &str, indent: usize) {
        let kind = node.kind();
        let start = node.start_byte();
        let end = node.end_byte();
        let text: String = source.as_bytes()[start..end]
            .iter()
            .map(|&b| if b.is_ascii() { b as char } else { '?' })
            .collect();
        let preview = if text.len() > 50 {
            format!("{}...", &text[..50])
        } else {
            text
        };
        eprintln!("{}{:?} [{}..{}] {:?}", "  ".repeat(indent), kind, start, end, preview);
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                print_node(child, source, indent + 1);
            }
        }
    }
    print_node(tree.root_node(), source, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let tree = parse("def foo(): return 1").unwrap();
        assert_eq!(tree.root_node().kind(), "module");
    }

    #[test]
    fn test_parse_jsx() {
        let tree = parse(r#"def App(): return <div>Hello</div>"#).unwrap();
        assert_eq!(tree.root_node().kind(), "module");
    }
}
