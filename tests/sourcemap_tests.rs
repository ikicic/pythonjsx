//! Source map tests - port of Python test_sourcemap.py

use pythonjsx::compiler::sourcemap::{LineColumnMap, MapResult, SourceMap, SourceMapNode};

#[test]
fn test_line_column_map_single_line() {
    let lcm = LineColumnMap::new("hello");
    assert_eq!(lcm.line_count(), 1);
    assert_eq!(lcm.byte_to_line_col(0), (0, 0));
    assert_eq!(lcm.byte_to_line_col(4), (0, 4));
    assert_eq!(lcm.line_col_to_byte(0, 0), 0);
    assert_eq!(lcm.line_col_to_byte(0, 4), 4);
}

#[test]
fn test_line_column_map_multi_line() {
    let lcm = LineColumnMap::new("ab\ncd\nef");
    assert_eq!(lcm.line_count(), 3);
    assert_eq!(lcm.byte_to_line_col(0), (0, 0));
    assert_eq!(lcm.byte_to_line_col(1), (0, 1));
    assert_eq!(lcm.byte_to_line_col(2), (0, 2));
    assert_eq!(lcm.byte_to_line_col(3), (1, 0));
    assert_eq!(lcm.byte_to_line_col(5), (1, 2));
    assert_eq!(lcm.byte_to_line_col(6), (2, 0));
    assert_eq!(lcm.byte_to_line_col(7), (2, 1));
    assert_eq!(lcm.line_col_to_byte(0, 0), 0);
    assert_eq!(lcm.line_col_to_byte(1, 0), 3);
    assert_eq!(lcm.line_col_to_byte(2, 0), 6);
    assert_eq!(lcm.line_col_to_byte(2, 1), 7);
}

#[test]
fn test_line_column_map_empty_string() {
    let lcm = LineColumnMap::new("");
    assert_eq!(lcm.line_count(), 1);
    assert_eq!(lcm.byte_to_line_col(0), (0, 0));
    assert_eq!(lcm.line_col_to_byte(0, 0), 0);
}

#[test]
fn test_source_map_node_leaf_identity() {
    let node = SourceMapNode {
        px_start: 0,
        px_end: 5,
        py_start: 10,
        py_end: 15,
        children: vec![],
    };
    assert!(node.is_leaf());
    assert_eq!(node.px_length(), 5);
    assert_eq!(node.py_length(), 5);
    assert!(node.is_identity());
}

#[test]
fn test_source_map_identity_leaf_py_to_px() {
    let root = SourceMapNode {
        px_start: 10,
        px_end: 15,
        py_start: 20,
        py_end: 25,
        children: vec![],
    };
    let sm = SourceMap::new(root);
    let result = sm.py_to_px(22);
    assert_eq!(result, MapResult { start: 12, end: 13, is_exact: true });
}

#[test]
fn test_source_map_identity_leaf_px_to_py() {
    let root = SourceMapNode {
        px_start: 10,
        px_end: 15,
        py_start: 20,
        py_end: 25,
        children: vec![],
    };
    let sm = SourceMap::new(root);
    let result = sm.px_to_py(12);
    assert_eq!(result, MapResult { start: 22, end: 23, is_exact: true });
}
