# JSXTemplate / JSXResult — template protocol runtime.
#
# The compiler hoists each JSX expression to a module-level
# `_tpl_N = JSXTemplate(...)` whose varargs mix compile-time `str` literals,
# `SLOT_VALUE` / `SLOT_SPREAD` sentinels, and `SlotAttr(name)` markers.
# `_tpl_N(args)` returns a `JSXResult(template, args)`; `.to_html()` walks
# the precompiled instruction program once into a `_PyUnicodeWriter`.
#
# Why this shape:
# * Templates are built once at import; per-render walks a flat array.
# * INSTR_LITERAL holds a borrowed pointer to a fused literal — no per-
#   chunk type check, straight to `_PyUnicodeWriter_WriteStr`.

cdef enum:
    INSTR_LITERAL = 0   # emit precompiled static bytes
    INSTR_VALUE   = 1   # render `args[slot_idx]` as escaped content
    INSTR_ATTR    = 2   # render ` name="value"` for compile-time-known name
    INSTR_SPREAD  = 3   # render dict/mapping as ` k="v"` pairs


cdef struct Instr:
    int op
    # INSTR_VALUE/ATTR/SPREAD: index into the call's args tuple.  Unused
    # for INSTR_LITERAL.
    int slot_idx
    # Borrowed `str*` (lifetime held by the template's `_refs` tuple):
    #   INSTR_LITERAL → fused literal str, written verbatim.
    #   INSTR_ATTR    → attr name str (grammar-restricted, no escaping).
    #   INSTR_VALUE / INSTR_SPREAD → NULL.
    PyObject* obj


# --- Sentinels ------------------------------------------------------------

cdef class _SlotValueSentinel:
    """Marker for a dynamic content slot.  Use the singleton `SLOT_VALUE`."""
    def __repr__(self):
        return "SLOT_VALUE"


cdef class _SlotSpreadSentinel:
    """Marker for a `{**expr}` spread slot."""
    def __repr__(self):
        return "SLOT_SPREAD"


SLOT_VALUE = _SlotValueSentinel()
SLOT_SPREAD = _SlotSpreadSentinel()


@cython.final
cdef class SlotAttr:
    """Slot for a dynamic attribute with a compile-time-known name:
    `<div id={expr}>` → `SlotAttr("id")`.  The name is grammar-restricted
    to identifier-like chars, so it's emitted verbatim (no escaping)."""
    cdef readonly str name

    def __cinit__(self, str name):
        self.name = name

    def __repr__(self):
        return f"SlotAttr({self.name!r})"


# --- SafeStr --------------------------------------------------------------

@cython.final
cdef class SafeStr:
    """Wraps a `str` as already HTML-safe. Intended for strings produced by
    trusted rendering or explicit pre-escaping."""
    cdef readonly str s

    def __cinit__(self, str s):
        self.s = s

    def __repr__(self):
        return f"SafeStr({self.s!r})"


# --- JSXTemplate ----------------------------------------------------------

@cython.final
cdef class JSXTemplate:
    """Module-level immutable program; `__call__` returns a `JSXResult`."""
    cdef Instr* _program
    cdef int _n_instrs
    cdef int _n_slots
    cdef Py_ssize_t _static_bytes
    # Lifetime anchor for the borrowed pointers in `Instr.obj`.
    cdef tuple _refs

    def __cinit__(self, *chunks):
        self._program = NULL
        self._n_instrs = 0
        self._n_slots = 0
        self._static_bytes = 0
        self._refs = ()
        self._build_program(chunks)

    def __dealloc__(self):
        if self._program != NULL:
            PyMem_Free(self._program)
            self._program = NULL

    def __call__(self, *args):
        if PyTuple_GET_SIZE(args) != self._n_slots:
            raise PythonJSXTypeError(
                f"JSXTemplate takes {self._n_slots} positional arg(s) "
                f"but {PyTuple_GET_SIZE(args)} were given"
            )
        return JSXResult._make(self, args)

    def __repr__(self):
        return (
            f"JSXTemplate(n_instrs={self._n_instrs}, "
            f"n_slots={self._n_slots}, static_bytes={self._static_bytes})"
        )

    @property
    def n_instrs(self) -> int:
        return self._n_instrs

    @property
    def n_slots(self) -> int:
        return self._n_slots

    @property
    def static_bytes(self) -> int:
        return self._static_bytes

    def _build_program(self, tuple chunks):
        """Build the Instr[] program: fuse adjacent literal strs, decode
        sentinels.  Runs once per template at import."""
        cdef list pending_lits = []
        cdef list refs = []
        cdef list raw = []
        cdef int slot_idx = 0
        cdef Py_ssize_t static_bytes = 0
        cdef object chunk

        def flush_pending():
            nonlocal static_bytes, pending_lits
            if not pending_lits:
                return
            fused = pending_lits[0] if len(pending_lits) == 1 else "".join(pending_lits)
            refs.append(fused)
            raw.append((INSTR_LITERAL, fused))
            static_bytes += PyUnicode_GET_LENGTH(fused)
            pending_lits = []

        for chunk in chunks:
            if isinstance(chunk, str):
                pending_lits.append(chunk)
            elif isinstance(chunk, _SlotValueSentinel):
                flush_pending()
                raw.append((INSTR_VALUE, slot_idx))
                slot_idx += 1
            elif isinstance(chunk, _SlotSpreadSentinel):
                flush_pending()
                raw.append((INSTR_SPREAD, slot_idx))
                slot_idx += 1
            elif isinstance(chunk, SlotAttr):
                # Unwrap to the bare name str — keeps Instr.obj uniformly
                # str (either fused literal or attr name).
                flush_pending()
                refs.append((<SlotAttr>chunk).name)
                raw.append((INSTR_ATTR, slot_idx, (<SlotAttr>chunk).name))
                slot_idx += 1
            else:
                raise PythonJSXTypeError(
                    f"JSXTemplate: unexpected chunk of type "
                    f"{type(chunk).__name__!r} "
                    f"(expected str, SLOT_VALUE, SLOT_SPREAD, or SlotAttr)"
                )
        flush_pending()

        cdef int n = len(raw)
        # Allocate at least 1 so `_program != NULL` always means initialized
        # (the walker bounds on `_n_instrs`).
        cdef size_t alloc_n = n if n > 0 else 1
        cdef Instr* program = <Instr*>PyMem_Malloc(alloc_n * sizeof(Instr))
        if program == NULL:
            raise MemoryError()

        cdef tuple refs_tuple
        cdef int i
        cdef int op
        cdef Instr* ip
        cdef str lit
        cdef str attr_name

        try:
            refs_tuple = tuple(refs)

            # Debug: validate types in raw instructions
            if PJR_DEBUG:
                for i in range(n):
                    row = raw[i]
                    op = row[0]
                    if op == INSTR_LITERAL:
                        assert PyUnicode_Check(row[1]), \
                            f"INSTR_LITERAL requires str, got {type(row[1]).__name__}"
                    elif op == INSTR_ATTR:
                        assert PyUnicode_Check(row[2]), \
                            f"INSTR_ATTR name requires str, got {type(row[2]).__name__}"

            for i in range(n):
                ip = &program[i]
                row = raw[i]
                op = row[0]
                ip.op = op
                ip.slot_idx = -1
                ip.obj = NULL
                if op == INSTR_LITERAL:
                    lit = row[1]
                    ip.obj = <PyObject*>lit
                elif op == INSTR_VALUE:
                    ip.slot_idx = row[1]
                elif op == INSTR_SPREAD:
                    ip.slot_idx = row[1]
                else:  # INSTR_ATTR
                    ip.slot_idx = row[1]
                    attr_name = row[2]
                    ip.obj = <PyObject*>attr_name

            self._program = program
            self._n_instrs = n
            self._n_slots = slot_idx
            self._static_bytes = static_bytes
            self._refs = refs_tuple
            program = NULL  # Prevent double-free on success

        except:
            # Clean up allocated memory on any error
            if program != NULL:
                PyMem_Free(program)
            raise


# --- JSXResult ------------------------------------------------------------

@cython.final
@cython.freelist(4096)
cdef class JSXResult:
    """Deferred (template, args) pair; rendering runs on `.to_html()` so
    nested composition doesn't build intermediate `str` objects.

    Fields are `PyObject*` (not typed cdef refs) to skip the auto-None
    init Cython would otherwise emit — saves four refcount ops per render."""
    cdef PyObject* _template
    cdef PyObject* _args

    @staticmethod
    cdef JSXResult _make(JSXTemplate template, tuple args):
        cdef JSXResult r = JSXResult.__new__(JSXResult)
        Py_INCREF(template)
        r._template = <PyObject*>template
        Py_INCREF(args)
        r._args = <PyObject*>args
        return r

    def __dealloc__(self):
        Py_XDECREF(self._template)
        Py_XDECREF(self._args)

    def to_html(self):
        cdef _PyUnicodeWriter writer
        _PyUnicodeWriter_Init(&writer)
        writer.overallocate = 1
        try:
            _jsxt_write_into(self._template, self._args, &writer)
        except:
            _PyUnicodeWriter_Dealloc(&writer)
            raise
        return _PyUnicodeWriter_Finish(&writer)

    def to_html_document(self, str prefix="<!DOCTYPE html>\n"):
        cdef _PyUnicodeWriter writer
        _PyUnicodeWriter_Init(&writer)
        writer.overallocate = 1
        try:
            _PyUnicodeWriter_WriteStr(&writer, prefix)
            _jsxt_write_into(self._template, self._args, &writer)
        except:
            _PyUnicodeWriter_Dealloc(&writer)
            raise
        return _PyUnicodeWriter_Finish(&writer)

    def __str__(self):
        return self.to_html()

    def __repr__(self):
        return "JSXResult(...)"


# --- Template execution ---------------------------------------------------

cdef int _jsxt_write_into(
    PyObject* tpl_obj, PyObject* args_obj, _PyUnicodeWriter* out,
) except -1:
    """Walk the instruction list.  `tpl_obj`/`args_obj` are borrowed so
    the caller skips INCREF/DECREF.  INSTR_VALUE inlines the two hot
    types (JSXResult, str) — composition is common and skipping the
    pjr_render_value call layer is measurable on loop-heavy shapes."""
    cdef JSXTemplate tpl = <JSXTemplate>tpl_obj
    cdef tuple args = <tuple>args_obj
    cdef int i
    cdef int n = tpl._n_instrs
    cdef Instr* ip
    cdef object value
    for i in range(n):
        ip = &tpl._program[i]
        if ip.op == INSTR_LITERAL:
            _PyUnicodeWriter_WriteStr(out, <object>ip.obj)
        elif ip.op == INSTR_VALUE:
            value = <object>PyTuple_GET_ITEM(args, ip.slot_idx)
            if type(value) is JSXResult:
                _jsxt_write_into(
                    (<JSXResult>value)._template,
                    (<JSXResult>value)._args,
                    out)
            elif PyUnicode_Check(value):
                _write_escaped_text(out, <str>value)
            else:
                pjr_render_value(value, out)
        elif ip.op == INSTR_ATTR:
            value = <object>PyTuple_GET_ITEM(args, ip.slot_idx)
            pjr_render_attr(<str>(<object>ip.obj), value, out)
        else:  # INSTR_SPREAD
            value = <object>PyTuple_GET_ITEM(args, ip.slot_idx)
            _pjr_unpack_attrs(value, out)
    return 0


# --- Slot-value type dispatcher -------------------------------------------

cdef int pjr_render_value(object value, _PyUnicodeWriter* out) except -1:
    """Render `value` as escaped HTML content.  Handles str / int / float /
    None / bool / JSXResult / bytes / SafeStr / iterable."""
    if PyUnicode_Check(value):
        return _write_escaped_text(out, <str>value)
    if type(value) is JSXResult:
        return _jsxt_write_into(
            (<JSXResult>value)._template,
            (<JSXResult>value)._args,
            out)
    if value is None:
        return 0
    if PyBool_Check(value):
        return 0
    if PyLong_Check(value) or isinstance(value, float):
        # Numeric repr has no HTML specials — skip escaping.
        return _PyUnicodeWriter_WriteStr(out, str(value))
    if PyBytes_Check(value):
        return _write_escaped_text(out, str(value))
    try:
        it = iter(value)
    except TypeError:
        if type(value) is SafeStr:
            return _PyUnicodeWriter_WriteStr(out, (<SafeStr>value).s)
        return _write_escaped_text(out, str(value))
    # Inline hot yield types so each iteration skips the recursive call.
    for item in it:
        if type(item) is JSXResult:
            _jsxt_write_into(
                (<JSXResult>item)._template,
                (<JSXResult>item)._args,
                out)
        elif PyUnicode_Check(item):
            _write_escaped_text(out, <str>item)
        else:
            pjr_render_value(item, out)
    return 0


# --- Attribute renderers --------------------------------------------------

cdef inline int _pjr_emit_attr_kv(
    str name_or_escaped, str v, _PyUnicodeWriter* out,
) except -1:
    """Emit ` name="value"`.  `name_or_escaped` must already be HTML-safe
    (static for INSTR_ATTR, attr-escaped for INSTR_SPREAD)."""
    _PyUnicodeWriter_WriteStr(out, _SP)
    _PyUnicodeWriter_WriteStr(out, name_or_escaped)
    _PyUnicodeWriter_WriteStr(out, _EQ_QUOT)
    _PyUnicodeWriter_WriteStr(out, v)
    _PyUnicodeWriter_WriteStr(out, _CLOSE_Q)
    return 0


cdef inline int _pjr_emit_attr_bare(str name, _PyUnicodeWriter* out) except -1:
    """Emit ` name` for `attr={True}`."""
    _PyUnicodeWriter_WriteStr(out, _SP)
    _PyUnicodeWriter_WriteStr(out, name)
    return 0


cdef int pjr_render_attr(
    str name, object value, _PyUnicodeWriter* out,
) except -1:
    """INSTR_ATTR with a compile-time-known name (no name-side escaping
    needed since grammar restricts it to identifier-like chars)."""
    if PyUnicode_Check(value):
        return _pjr_emit_attr_kv(name, _escape_attr_str(<str>value), out)
    if value is None:
        return 0
    if PyBool_Check(value):
        if value is True:
            return _pjr_emit_attr_bare(name, out)
        return 0
    if type(value) is SafeStr:
        return _pjr_emit_attr_kv(name, (<SafeStr>value).s, out)
    return _pjr_emit_attr_kv(name, _escape_attr_str(str(value)), out)


cdef inline int _pjr_emit_one_attr_escaped(
    object k, object v, _PyUnicodeWriter* out,
) except -1:
    """Spread-path key/value pair: keys validated, values attr-escaped."""
    if not PyUnicode_Check(k):
        raise PythonJSXTypeError("spread: attribute name must be str")
    _check_attr_name(<str>k)
    if PyUnicode_Check(v):
        return _pjr_emit_attr_kv(<str>k, _escape_attr_str(<str>v), out)
    if v is None:
        return 0
    if PyBool_Check(v):
        if v is True:
            return _pjr_emit_attr_bare(<str>k, out)
        return 0
    if type(v) is SafeStr:
        return _pjr_emit_attr_kv(<str>k, (<SafeStr>v).s, out)
    return _pjr_emit_attr_kv(<str>k, _escape_attr_str(str(v)), out)


cdef int _pjr_unpack_attrs(object attrs, _PyUnicodeWriter* out) except -1:
    """Render `{**attrs}`. Dict fast-path via PyDict_Next; fallback to
    `.keys()` + `__getitem__` for arbitrary mappings."""
    cdef Py_ssize_t pos = 0
    cdef PyObject* pk
    cdef PyObject* pv
    cdef object k, v
    if PyDict_Check(attrs):
        while PyDict_Next(<dict>attrs, &pos, &pk, &pv):
            _pjr_emit_one_attr_escaped(<object>pk, <object>pv, out)
        return 0
    for k in attrs.keys():
        v = attrs[k]
        _pjr_emit_one_attr_escaped(k, v, out)
    return 0
