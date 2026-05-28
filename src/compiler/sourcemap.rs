//! Hierarchical source map for .px <-> .py position mapping.

/// One source-map node: maps `[px_start, px_end)` in .px source to
/// `[py_start, py_end)` in .py output.
#[derive(Debug, Clone)]
pub struct SourceMapNode {
    pub px_start: usize,
    pub px_end: usize,
    pub py_start: usize,
    pub py_end: usize,
    pub children: Vec<SourceMapNode>,
}

impl SourceMapNode {
    pub fn px_length(&self) -> usize {
        self.px_end.saturating_sub(self.px_start)
    }

    pub fn py_length(&self) -> usize {
        self.py_end.saturating_sub(self.py_start)
    }

    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    pub fn is_identity(&self) -> bool {
        self.px_length() == self.py_length()
    }
}

/// Result of mapping a byte offset between .px and .py.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapResult {
    pub start: usize,
    pub end: usize,
    pub is_exact: bool,
}

/// Bidirectional source map between .px and .py byte offsets.
#[derive(Debug, Clone)]
pub struct SourceMap {
    pub root: SourceMapNode,
}

impl SourceMap {
    pub fn new(root: SourceMapNode) -> Self {
        Self { root }
    }

    pub fn py_to_px(&self, py_offset: usize) -> MapResult {
        lookup(&self.root, py_offset, true)
    }

    pub fn px_to_py(&self, px_offset: usize) -> MapResult {
        lookup(&self.root, px_offset, false)
    }
}

// TODO: performance — `lookup` does a linear scan of `children` at every
// recursion level. For small trees this is fine, but the LSP remaps hundreds
// of ranges per diagnostics batch (once per pyright diagnostic) and the
// children are already sorted by construction. Swap for binary search, or
// build an interval tree, once the profile shows this is a real cost.
fn lookup(node: &SourceMapNode, offset: usize, py_to_px: bool) -> MapResult {
    let (src_start, src_end, tgt_start, tgt_end) = if py_to_px {
        (node.py_start, node.py_end, node.px_start, node.px_end)
    } else {
        (node.px_start, node.px_end, node.py_start, node.py_end)
    };

    if node.is_leaf() {
        if node.is_identity() && offset >= src_start && offset < src_end {
            let mapped = tgt_start + (offset - src_start);
            return MapResult {
                start: mapped,
                end: mapped + 1,
                is_exact: true,
            };
        }
        return MapResult {
            start: tgt_start,
            end: tgt_end,
            is_exact: false,
        };
    }

    for child in &node.children {
        let (child_start, child_end) = if py_to_px {
            (child.py_start, child.py_end)
        } else {
            (child.px_start, child.px_end)
        };
        if offset >= child_start && offset < child_end {
            return lookup(child, offset, py_to_px);
        }
    }

    MapResult {
        start: tgt_start,
        end: tgt_end,
        is_exact: false,
    }
}

/// Byte-offset ↔ (line, col) mapping.  0-based; columns are UTF-8 bytes.
#[derive(Debug, Clone)]
pub struct LineColumnMap {
    line_starts: Vec<usize>,
    _total_bytes: usize,
}

impl LineColumnMap {
    pub fn new(source: &str) -> Self {
        let source_bytes = source.as_bytes();
        let mut line_starts = vec![0];
        for (i, &b) in source_bytes.iter().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            line_starts,
            _total_bytes: source_bytes.len(),
        }
    }

    pub fn byte_to_line_col(&self, byte_offset: usize) -> (usize, usize) {
        let idx = self.line_starts.partition_point(|&s| s <= byte_offset);
        let line = idx.saturating_sub(1).min(self.line_starts.len().saturating_sub(1));
        let col = byte_offset.saturating_sub(self.line_starts[line]);
        (line, col)
    }

    pub fn line_col_to_byte(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.line_starts.len().saturating_sub(1));
        self.line_starts[line] + col
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}
