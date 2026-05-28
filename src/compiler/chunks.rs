//! Flat chunk representation of compiled JSX expressions.
//!
//! Each JSX expression compiles to a sequence of `Chunk`s — Static HTML or
//! Dynamic Python expression.  `Chunks` keeps adjacent statics merged, but
//! lazily (via `pending_static`) so total byte copying stays O(output).
//!
//! `as_template_call` decodes the sequence into a template + call-site args
//! pair — see the per-shape table in its doc comment.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    /// Pre-escaped HTML string emitted verbatim at runtime.
    Static(String),
    /// Python expression yielding a renderable value (str, JSXResult, …).
    Dynamic(String),
}

/// A flat sequence of chunks with deferred static merging.
#[derive(Debug, Default)]
pub struct Chunks {
    chunks: Vec<Chunk>,
    pending_static: Vec<String>,
}

impl Chunks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_static(&mut self, s: impl Into<String>) {
        let s: String = s.into();
        if s.is_empty() {
            return;
        }
        self.pending_static.push(s);
    }

    pub fn push_dynamic(&mut self, expr: impl Into<String>) {
        self.flush_pending_static();
        self.chunks.push(Chunk::Dynamic(expr.into()));
    }

    /// Append `other`, keeping border statics pending so they fuse on next flush.
    pub fn extend(&mut self, other: Chunks) {
        for chunk in other.chunks.into_iter() {
            match chunk {
                Chunk::Static(s) => self.pending_static.push(s),
                Chunk::Dynamic(e) => {
                    self.flush_pending_static();
                    self.chunks.push(Chunk::Dynamic(e));
                }
            }
        }
        self.pending_static.extend(other.pending_static);
    }

    // TODO: performance — every flush allocates a fresh `String::with_capacity`.
    // For hot paths (large documents with many dynamic chunks) a reusable
    // scratch buffer owned by `Chunks` could be cheaper. Measure with
    // `make benchmark-compile` before rewriting.
    fn flush_pending_static(&mut self) {
        if self.pending_static.is_empty() {
            return;
        }
        // One exact-capacity concat covering the prior trailing Static (if
        // any — defensive; invariant says there isn't one) and pending pieces.
        let pending = std::mem::take(&mut self.pending_static);
        let prev_trailing = match self.chunks.last() {
            Some(Chunk::Static(_)) => {
                if let Some(Chunk::Static(s)) = self.chunks.pop() {
                    Some(s)
                } else {
                    unreachable!()
                }
            }
            _ => None,
        };
        let cap: usize = prev_trailing.as_ref().map_or(0, |s| s.len())
            + pending.iter().map(|s| s.len()).sum::<usize>();
        let mut merged = String::with_capacity(cap);
        if let Some(s) = prev_trailing {
            merged.push_str(&s);
        }
        for p in &pending {
            merged.push_str(p);
        }
        if !merged.is_empty() {
            self.chunks.push(Chunk::Static(merged));
        }
    }

    /// Flush pending and return the final chunk list.
    pub fn finish(mut self) -> Vec<Chunk> {
        self.flush_pending_static();
        self.chunks
    }

}

fn py_literal(s: &str) -> String {
    // Rust's Debug format yields a Python-compatible literal (ASCII) with
    // non-ASCII escaped via `\u{…}`.
    format!("{:?}", s)
}

/// Which runtime slot markers a template references; ORed into the emitter
/// so the preamble imports only what's used (no spurious "unused import").
#[derive(Debug, Default, Clone, Copy)]
pub struct TemplateUsage {
    pub slot_value: bool,
    pub slot_spread: bool,
    pub slot_attr: bool,
}

/// Walk a chunk sequence and produce `(template_args, call_args, usage)`
/// for the template protocol.  Opcode-interleaved dynamic chunks (the
/// `OP_*` integer followed by its operands) are collapsed into one slot
/// marker in the template plus one or more positional values at the
/// call site:
///
/// | chunk shape                      | template marker | call-site arg |
/// |----------------------------------|-----------------|----------------|
/// | `Static(s)`                      | `"s"`           | —              |
/// | `Dynamic("0"), Dynamic(expr)`    | `_pjr_V`        | `expr`         |
/// | `Dynamic("3"), Dynamic(expr)`    | `_pjr_V`        | `expr`         |
/// | `Dynamic("2"), Dynamic(expr)`    | `_pjr_S`        | `expr`         |
/// | `Dynamic("1"), "name_lit", expr` | `_pjr_A(name)`  | `expr`         |
/// | `Dynamic(expr)` (bare)           | `_pjr_V`        | `expr`         |
///
/// Where `"0"`…`"3"` are the `OP_*` opcode values the visitor pushes.
/// `"name_lit"` is an already-quoted Python string literal like `"id"`.
pub fn as_template_call(chunks: &[Chunk]) -> (String, String, TemplateUsage) {
    use crate::compiler::opcodes::{
        OP_ESCAPE_TEXT, OP_RENDER_ATTR, OP_UNPACK_ARGS, OP_UNPACK_ATTRS,
    };
    let mut template_parts: Vec<String> = Vec::new();
    let mut call_parts: Vec<String> = Vec::new();
    let mut usage = TemplateUsage::default();
    let op_escape = OP_ESCAPE_TEXT.to_string();
    let op_render_attr = OP_RENDER_ATTR.to_string();
    let op_unpack_attrs = OP_UNPACK_ATTRS.to_string();
    let op_unpack_args = OP_UNPACK_ARGS.to_string();
    let mut i = 0;
    while i < chunks.len() {
        match &chunks[i] {
            Chunk::Static(s) => {
                template_parts.push(py_literal(s));
                i += 1;
            }
            Chunk::Dynamic(d) => {
                if d == &op_escape || d == &op_unpack_args {
                    // Both → SLOT_VALUE; pjr_render_value handles single + iterable.
                    template_parts.push("_pjr_V".to_string());
                    usage.slot_value = true;
                    if let Chunk::Dynamic(expr) = &chunks[i + 1] {
                        call_parts.push(expr.clone());
                    }
                    i += 2;
                } else if d == &op_unpack_attrs {
                    template_parts.push("_pjr_S".to_string());
                    usage.slot_spread = true;
                    if let Chunk::Dynamic(expr) = &chunks[i + 1] {
                        call_parts.push(expr.clone());
                    }
                    i += 2;
                } else if d == &op_render_attr {
                    // Name operand is already a Python str literal (e.g. `"id"`)
                    // — pass it as-is to `SlotAttr(...)`.
                    if let (Chunk::Dynamic(name_lit), Chunk::Dynamic(value)) =
                        (&chunks[i + 1], &chunks[i + 2])
                    {
                        template_parts.push(format!("_pjr_A({})", name_lit));
                        usage.slot_attr = true;
                        call_parts.push(value.clone());
                    }
                    i += 3;
                } else {
                    // Bare dynamic (e.g. `Foo(...)` component call) → SLOT_VALUE.
                    template_parts.push("_pjr_V".to_string());
                    usage.slot_value = true;
                    call_parts.push(d.clone());
                    i += 1;
                }
            }
        }
    }
    (template_parts.join(", "), call_parts.join(", "), usage)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Chunk { Chunk::Static(x.to_string()) }
    fn d(x: &str) -> Chunk { Chunk::Dynamic(x.to_string()) }

    #[test]
    fn static_chunks_merge_on_flush() {
        let mut c = Chunks::new();
        c.push_static("a");
        c.push_static("b");
        c.push_static("c");
        assert_eq!(c.finish(), vec![s("abc")]);
    }

    #[test]
    fn dynamic_flushes_pending_static() {
        let mut c = Chunks::new();
        c.push_static("<div>");
        c.push_static("Hello ");
        c.push_dynamic("render_text(x)");
        c.push_static("!");
        c.push_static("</div>");
        assert_eq!(
            c.finish(),
            vec![s("<div>Hello "), d("render_text(x)"), s("!</div>")]
        );
    }

    #[test]
    fn extend_merges_border_statics() {
        let mut outer = Chunks::new();
        outer.push_static("A");
        let mut inner = Chunks::new();
        inner.push_static("B");
        inner.push_dynamic("x");
        inner.push_static("C");
        outer.extend(inner);
        outer.push_static("D");
        assert_eq!(outer.finish(), vec![s("AB"), d("x"), s("CD")]);
    }

    #[test]
    fn empty_statics_are_dropped() {
        let mut c = Chunks::new();
        c.push_static("");
        c.push_static("hello");
        c.push_static("");
        assert_eq!(c.finish(), vec![s("hello")]);
    }

    #[test]
    fn as_template_call_formats_correctly() {
        // Static + OP_ESCAPE_TEXT (0) + expr + static
        let chunks = vec![s("<div>"), d("0"), d("x"), s("</div>")];
        let (tpl, call, usage) = as_template_call(&chunks);
        assert_eq!(tpl, r#""<div>", _pjr_V, "</div>""#);
        assert_eq!(call, "x");
        assert!(usage.slot_value && !usage.slot_spread && !usage.slot_attr);
    }

    #[test]
    fn as_template_call_attr() {
        // Static + OP_RENDER_ATTR (1) + name-literal + value-expr + static
        let chunks = vec![s("<a"), d("1"), d(r#""href""#), d("u"), s(">")];
        let (tpl, call, usage) = as_template_call(&chunks);
        assert_eq!(tpl, r#""<a", _pjr_A("href"), ">""#);
        assert_eq!(call, "u");
        assert!(!usage.slot_value && !usage.slot_spread && usage.slot_attr);
    }

    #[test]
    fn as_template_call_spread_and_bare_dynamic() {
        // OP_UNPACK_ATTRS (2) and a bare dynamic (e.g. a component call)
        let chunks = vec![s("<"), d("2"), d("a"), d("Foo()")];
        let (tpl, call, usage) = as_template_call(&chunks);
        assert_eq!(tpl, r#""<", _pjr_S, _pjr_V"#);
        assert_eq!(call, "a, Foo()");
        assert!(usage.slot_value && usage.slot_spread && !usage.slot_attr);
    }
}
