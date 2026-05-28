# HTML-escape helpers — two-pass (count, fill), per-kind specialized.

cdef Py_ssize_t _count_text(const _ucs_t* data, Py_ssize_t length) noexcept nogil:
    cdef Py_ssize_t i
    cdef Py_ssize_t n_amp = 0
    cdef Py_ssize_t n_lt_gt = 0
    cdef _ucs_t ch
    for i in range(length):
        # Branchless: helps the C compiler vectorize.
        ch = data[i]
        n_amp += (ch == _AMP)
        n_lt_gt += (ch == _LT) | (ch == _GT)
    # `&`→`&amp;` (+4), `<`/`>`→`&lt;`/`&gt;` (+3 each).
    return 4 * n_amp + 3 * n_lt_gt


cdef Py_ssize_t _count_attr(const _ucs_t* data, Py_ssize_t length) noexcept nogil:
    cdef Py_ssize_t i
    cdef Py_ssize_t n_amp = 0
    cdef Py_ssize_t n_lt_gt = 0
    cdef Py_ssize_t n_quot = 0
    cdef _ucs_t ch
    for i in range(length):
        ch = data[i]
        n_amp += (ch == _AMP)
        n_lt_gt += (ch == _LT) | (ch == _GT)
        n_quot += (ch == _QUOT)
    # `"`→`&quot;` (+5).
    return 4 * n_amp + 3 * n_lt_gt + 5 * n_quot


cdef void _fill_text(const _ucs_t* src, Py_ssize_t length, _ucs_t* dst) noexcept nogil:
    cdef Py_ssize_t i, j = 0
    cdef _ucs_t ch
    for i in range(length):
        ch = src[i]
        if ch == _AMP:
            dst[j]     = _AMP
            dst[j + 1] = _A
            dst[j + 2] = _M
            dst[j + 3] = _P
            dst[j + 4] = _SEMI
            j += 5
        elif ch == _LT:
            dst[j]     = _AMP
            dst[j + 1] = _L
            dst[j + 2] = _T
            dst[j + 3] = _SEMI
            j += 4
        elif ch == _GT:
            dst[j]     = _AMP
            dst[j + 1] = _G
            dst[j + 2] = _T
            dst[j + 3] = _SEMI
            j += 4
        else:
            dst[j] = ch
            j += 1


cdef void _fill_attr(const _ucs_t* src, Py_ssize_t length, _ucs_t* dst) noexcept nogil:
    cdef Py_ssize_t i, j = 0
    cdef _ucs_t ch
    for i in range(length):
        ch = src[i]
        if ch == _AMP:
            dst[j]     = _AMP
            dst[j + 1] = _A
            dst[j + 2] = _M
            dst[j + 3] = _P
            dst[j + 4] = _SEMI
            j += 5
        elif ch == _LT:
            dst[j]     = _AMP
            dst[j + 1] = _L
            dst[j + 2] = _T
            dst[j + 3] = _SEMI
            j += 4
        elif ch == _GT:
            dst[j]     = _AMP
            dst[j + 1] = _G
            dst[j + 2] = _T
            dst[j + 3] = _SEMI
            j += 4
        elif ch == _QUOT:
            dst[j]     = _AMP
            dst[j + 1] = _Q
            dst[j + 2] = _U
            dst[j + 3] = _O
            dst[j + 4] = _T
            dst[j + 5] = _SEMI
            j += 6
        else:
            dst[j] = ch
            j += 1


cdef str _escape_text_str(str s):
    cdef Py_ssize_t length = PyUnicode_GET_LENGTH(s)
    cdef int kind = PyUnicode_KIND(s)
    cdef void* data = PyUnicode_DATA(s)
    cdef Py_ssize_t extra

    if kind == 1:
        extra = _count_text[uint8_t](<const uint8_t*>data, length)
    elif kind == 2:
        extra = _count_text[uint16_t](<const uint16_t*>data, length)
    else:  # kind == 4
        extra = _count_text[uint32_t](<const uint32_t*>data, length)
    if extra == 0:
        return s

    cdef object result = PyUnicode_New(length + extra, PyUnicode_MAX_CHAR_VALUE(s))
    cdef void* rdata = PyUnicode_DATA(result)
    if kind == 1:
        _fill_text[uint8_t](<const uint8_t*>data, length, <uint8_t*>rdata)
    elif kind == 2:
        _fill_text[uint16_t](<const uint16_t*>data, length, <uint16_t*>rdata)
    else:
        _fill_text[uint32_t](<const uint32_t*>data, length, <uint32_t*>rdata)
    return result


cdef str _escape_attr_str(str s):
    cdef Py_ssize_t length = PyUnicode_GET_LENGTH(s)
    cdef int kind = PyUnicode_KIND(s)
    cdef void* data = PyUnicode_DATA(s)
    cdef Py_ssize_t extra

    if kind == 1:
        extra = _count_attr[uint8_t](<const uint8_t*>data, length)
    elif kind == 2:
        extra = _count_attr[uint16_t](<const uint16_t*>data, length)
    else:
        extra = _count_attr[uint32_t](<const uint32_t*>data, length)
    if extra == 0:
        return s

    cdef object result = PyUnicode_New(length + extra, PyUnicode_MAX_CHAR_VALUE(s))
    cdef void* rdata = PyUnicode_DATA(result)
    if kind == 1:
        _fill_attr[uint8_t](<const uint8_t*>data, length, <uint8_t*>rdata)
    elif kind == 2:
        _fill_attr[uint16_t](<const uint16_t*>data, length, <uint16_t*>rdata)
    else:
        _fill_attr[uint32_t](<const uint32_t*>data, length, <uint32_t*>rdata)
    return result


# Forbidden in attr names: C0 controls, space, `"`, `'`, `&`, `/`, `<`, `=`, `>`, DEL.
cdef int _any_invalid_attr_name_char(
    const _ucs_t* data, Py_ssize_t length,
) noexcept nogil:
    cdef Py_ssize_t i
    cdef int bad = 0
    cdef _ucs_t ch
    for i in range(length):
        ch = data[i]
        bad |= (
            (ch <= 0x20)            # C0 controls + space
            | (ch == _QUOT)
            | (ch == _APOS)
            | (ch == _AMP)
            | (ch == _SLASH)
            | (ch == _LT)
            | (ch == _EQ)
            | (ch == _GT)
            | (ch == _DEL)
        )
    return bad


cdef int _check_attr_name(str name) except -1:
    """Raise ValueError if `name` is empty or contains a forbidden char."""
    cdef Py_ssize_t length = PyUnicode_GET_LENGTH(name)
    cdef int kind
    cdef void* data
    cdef int bad
    if length == 0:
        raise PythonJSXValueError("spread: attribute name must not be empty")
    kind = PyUnicode_KIND(name)
    data = PyUnicode_DATA(name)
    if kind == 1:
        bad = _any_invalid_attr_name_char[uint8_t](<const uint8_t*>data, length)
    elif kind == 2:
        bad = _any_invalid_attr_name_char[uint16_t](<const uint16_t*>data, length)
    else:
        bad = _any_invalid_attr_name_char[uint32_t](<const uint32_t*>data, length)
    if bad:
        raise PythonJSXValueError(
            f"spread: invalid character in attribute name {name!r}"
        )
    return 0


cdef int _write_escaped_text(_PyUnicodeWriter* out, str s) except -1:
    """Fused escape-and-write: reserve exact escaped-size space and fill
    in place.  Cross-kind falls back to the allocation path."""
    cdef Py_ssize_t length = PyUnicode_GET_LENGTH(s)
    cdef int src_kind = PyUnicode_KIND(s)
    cdef void* src_data = PyUnicode_DATA(s)
    cdef Py_UCS4 maxchar = PyUnicode_MAX_CHAR_VALUE(s)
    cdef Py_ssize_t extra

    if src_kind == 1:
        extra = _count_text[uint8_t](<const uint8_t*>src_data, length)
    elif src_kind == 2:
        extra = _count_text[uint16_t](<const uint16_t*>src_data, length)
    else:
        extra = _count_text[uint32_t](<const uint32_t*>src_data, length)

    if extra == 0:
        return _PyUnicodeWriter_WriteStr(out, s)

    _PyUnicodeWriter_Prepare(out, length + extra, maxchar)

    # Common case: writer.kind == src_kind (content stays at one kind).
    # Cross-kind: fall back to the allocation path.
    if out.kind != src_kind:
        return _PyUnicodeWriter_WriteStr(out, _escape_text_str(s))

    cdef void* dst = <uint8_t*>out.data + out.pos * out.kind
    if src_kind == 1:
        _fill_text[uint8_t](<const uint8_t*>src_data, length, <uint8_t*>dst)
    elif src_kind == 2:
        _fill_text[uint16_t](<const uint16_t*>src_data, length, <uint16_t*>dst)
    else:
        _fill_text[uint32_t](<const uint32_t*>src_data, length, <uint32_t*>dst)
    out.pos += length + extra
    return 0
