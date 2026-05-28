//! Opcodes shared with the Cython runtime.  Kept in sync with the `cdef
//! enum` and `VERSION` in `runtime-cy/_native_cy.pyx` — bump
//! `RUNTIME_ABI_VERSION` whenever opcode values or semantics change.

/// Matches `pythonjsx.runtime.VERSION`.
pub const RUNTIME_ABI_VERSION: u32 = 1;

/// `{expr}` in JSX text. Operand: a Python expression rendered as
/// HTML-escaped text (recurses into iterables/JSXResult; drops None/bool).
pub const OP_ESCAPE_TEXT: u32 = 0;

/// `<div class={cls}>` (compile-time-known name).  Operands:
/// `(name_literal, value_expr)`.  Emits ` name="escaped"` for str values,
/// ` name` for True, nothing for False/None.  Name is not re-escaped.
pub const OP_RENDER_ATTR: u32 = 1;

/// `{**expr}` spread.  Operand: dict or mapping.  Keys are attr-escaped
/// too, so a hostile key like `'><script>` becomes inert HTML.
pub const OP_UNPACK_ATTRS: u32 = 2;

/// Non-JSX-body generator `{expr for x in xs}`.  Operand: iterable; each
/// item is rendered like OP_ESCAPE_TEXT.  JSX-body generators take a
/// different (chunks-tuple) path.
pub const OP_UNPACK_ARGS: u32 = 3;
