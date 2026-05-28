//! Direct emission: output buffer + source map builder.

use crate::compiler::error::CompileError;
use crate::compiler::sourcemap::SourceMapNode;

struct StackFrame {
    px_start: usize,
    px_end: usize,
    py_start: usize,
    children: Vec<SourceMapNode>,
}

/// Emits output directly to a buffer while building the source map incrementally.
pub struct Emitter<'a> {
    pub output: Vec<u8>,
    stack: Vec<StackFrame>,
    pub errors: Vec<CompileError>,
    pub source: &'a [u8],
    /// Counter for `_pjr_tpl_N` template names.
    pub template_counter: usize,
    /// Accumulated `_pjr_tpl_N = _pjr_Tpl(...)` defs, spliced at module end
    /// so import-time expressions see the templates.
    pub template_defs: String,
    /// Output byte offset where the preamble (imports + ABI assert + defs)
    /// gets spliced; set at the first non-skippable module-level node.
    pub template_insert_pos: Option<usize>,
    /// Slot-kind usage flags so the preamble imports only what's used.
    pub uses_slot_value: bool,
    pub uses_slot_spread: bool,
    pub uses_slot_attr: bool,
}

impl<'a> Emitter<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            output: Vec::new(),
            stack: vec![StackFrame {
                px_start: 0,
                px_end: source.len(),
                py_start: 0,
                children: Vec::new(),
            }],
            errors: Vec::new(),
            source,
            template_counter: 0,
            template_defs: String::new(),
            template_insert_pos: None,
            uses_slot_value: false,
            uses_slot_spread: false,
            uses_slot_attr: false,
        }
    }

    /// Splice `bytes` into `output` at `at`, shifting later py offsets right.
    pub fn splice_in(&mut self, at: usize, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let delta = bytes.len();
        let mut new_output = Vec::with_capacity(self.output.len() + delta);
        new_output.extend_from_slice(&self.output[..at]);
        new_output.extend_from_slice(bytes);
        new_output.extend_from_slice(&self.output[at..]);
        self.output = new_output;

        fn shift(children: &mut Vec<SourceMapNode>, at: usize, delta: usize) {
            for child in children {
                if child.py_start >= at {
                    child.py_start += delta;
                }
                if child.py_end >= at {
                    child.py_end += delta;
                }
                shift(&mut child.children, at, delta);
            }
        }
        for frame in self.stack.iter_mut() {
            if frame.py_start >= at {
                frame.py_start += delta;
            }
            shift(&mut frame.children, at, delta);
        }
    }

    pub fn next_template_name(&mut self) -> String {
        let name = format!("_pjr_tpl_{}", self.template_counter);
        self.template_counter += 1;
        name
    }

    /// Record a `_pjr_tpl_N = _pjr_Tpl(...)` definition.  `eager` appends
    /// `()` so a slot-less template is pre-evaluated to a `JSXResult` at
    /// import time, letting call sites drop the `()` too.
    pub fn emit_template_def(&mut self, name: &str, template_args: &str, eager: bool) {
        use std::fmt::Write;
        let tail = if eager { "()" } else { "" };
        let _ = writeln!(
            self.template_defs,
            "{} = _pjr_Tpl({}){}",
            name, template_args, tail,
        );
    }

    fn py_len(&self) -> usize {
        self.output.len()
    }

    /// Copy source bytes verbatim, recording an identity source-map leaf.
    pub fn emit_verbatim(&mut self, px_start: usize, px_end: usize) {
        debug_assert!(
            px_start <= px_end,
            "emit_verbatim: px_start ({}) > px_end ({}) — caller passed an inverted range",
            px_start,
            px_end,
        );
        let px_end = px_end.min(self.source.len());
        if px_start >= px_end {
            return;
        }
        let py_start = self.py_len();
        self.output.extend_from_slice(&self.source[px_start..px_end]);
        let py_end = self.py_len();
        self.add_leaf(px_start, px_end, py_start, py_end);
    }

    /// Append generated text, recording the (px, py) range pair.
    pub fn emit_generated(&mut self, px_start: usize, px_end: usize, text: &str) {
        let py_start = self.py_len();
        self.output.extend_from_slice(text.as_bytes());
        let py_end = self.py_len();
        self.add_leaf(px_start, px_end, py_start, py_end);
    }

    fn add_leaf(&mut self, px_start: usize, px_end: usize, py_start: usize, py_end: usize) {
        let node = SourceMapNode {
            px_start,
            px_end,
            py_start,
            py_end,
            children: Vec::new(),
        };
        if let Some(frame) = self.stack.last_mut() {
            frame.children.push(node);
        }
    }

    pub fn open_region(&mut self, px_start: usize, px_end: usize) {
        let py_start = self.py_len();
        self.stack.push(StackFrame {
            px_start,
            px_end,
            py_start,
            children: Vec::new(),
        });
    }

    pub fn close_region(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let frame = self.stack.pop().unwrap();
        let py_end = self.py_len();
        let node = SourceMapNode {
            px_start: frame.px_start,
            px_end: frame.px_end,
            py_start: frame.py_start,
            py_end,
            children: frame.children,
        };
        if let Some(parent) = self.stack.last_mut() {
            parent.children.push(node);
        }
    }

    pub fn error(&mut self, message: impl Into<String>, px_start: usize, px_end: usize) {
        self.errors
            .push(CompileError::error(message, px_start..px_end));
    }

    pub fn warning(&mut self, message: impl Into<String>, px_start: usize, px_end: usize) {
        self.errors
            .push(CompileError::warning(message, px_start..px_end));
    }

    pub fn merge_errors(&mut self, errors: Vec<CompileError>) {
        self.errors.extend(errors);
    }

    pub fn finish(mut self) -> (SourceMapNode, Vec<CompileError>) {
        while self.stack.len() > 1 {
            self.close_region();
        }
        let root = self.stack.pop().unwrap();
        let py_end = self.py_len();
        let node = SourceMapNode {
            px_start: root.px_start,
            px_end: root.px_end,
            py_start: root.py_start,
            py_end,
            children: root.children,
        };
        (node, self.errors)
    }
}
