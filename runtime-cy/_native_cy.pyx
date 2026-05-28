# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False
# cython: embedsignature=False, initializedcheck=False
# distutils: language = c
"""Cython runtime for PythonJSX (private API; consumed only by compiler
output).  The compiler hoists each JSX expression to a module-level
`_pjr_tpl_N = _pjr_Tpl(...)` and emits `_pjr_tpl_N(args)` at call sites;
`.to_html()` walks the precompiled instruction list once.

Split via `include` (single .so, preserves cross-function inlining):
  * `escape.pxi`   — HTML-escape helpers + `_write_escaped_text`.
  * `template.pxi` — `JSXTemplate` / `JSXResult` + slot sentinels.
"""

import cython
from cpython.object cimport PyObject
from cpython.unicode cimport PyUnicode_Check
from cpython.tuple cimport PyTuple_GET_SIZE, PyTuple_GET_ITEM
from cpython.bool cimport PyBool_Check
from cpython.bytes cimport PyBytes_Check
from cpython.dict cimport PyDict_Check, PyDict_Next
from cpython.long cimport PyLong_Check
from libc.stdint cimport uint8_t, uint16_t, uint32_t
from cpython.mem cimport PyMem_Malloc, PyMem_Free
from cpython.ref cimport Py_INCREF, Py_XDECREF


# PJR_DEBUG: 0 by default (constant-folded out); `make runtime-cython-debug`
# rebuilds with -DPJR_DEBUG=1 to enable internal invariant checks.
cdef extern from *:
    """
    #ifndef PJR_DEBUG
    #define PJR_DEBUG 0
    #endif
    """
    int PJR_DEBUG


# `_PyUnicodeWriter`: stack-allocatable growable str buffer; lets us skip
# the `list[str]` + `"".join(...)` round-trip.  Deprecated in 3.14 in
# favor of heap-allocated `PyUnicodeWriter` but still supported (we pass
# `-Wno-deprecated-declarations`).
cdef extern from "Python.h":
    ctypedef struct _PyUnicodeWriter:
        PyObject *buffer
        void *data
        int kind
        Py_UCS4 maxchar
        Py_ssize_t size
        Py_ssize_t pos
        Py_ssize_t min_length
        Py_UCS4 min_char
        unsigned char overallocate
        unsigned char readonly

    void _PyUnicodeWriter_Init(_PyUnicodeWriter *writer)
    int _PyUnicodeWriter_WriteStr(_PyUnicodeWriter *writer, object s) except -1
    object _PyUnicodeWriter_Finish(_PyUnicodeWriter *writer)
    void _PyUnicodeWriter_Dealloc(_PyUnicodeWriter *writer)
    # Reserve `length` more chars; upgrades `writer->kind` to fit `maxchar`.
    int _PyUnicodeWriter_Prepare(
        _PyUnicodeWriter *writer, Py_ssize_t length, Py_UCS4 maxchar,
    ) except -1


# PEP 393 `str` storage: kind ∈ {1, 2, 4} bytes per code point.  We cast
# PyUnicode_DATA to the concrete pointer type and dispatch into one of
# three hand-specialized loops — the C compiler can vectorize the kind=1
# (Latin-1) fast path without a per-iteration branch.
cdef extern from "Python.h":
    int PyUnicode_KIND(object)
    void* PyUnicode_DATA(object)
    object PyUnicode_New(Py_ssize_t size, Py_UCS4 maxchar)
    Py_ssize_t PyUnicode_GET_LENGTH(object)
    Py_UCS4 PyUnicode_MAX_CHAR_VALUE(object)
    object PyUnicode_FromKindAndData(int kind, const void* buffer, Py_ssize_t size)


# Cython fused type: three specialized copies, one per kind width.
ctypedef fused _ucs_t:
    uint8_t
    uint16_t
    uint32_t


# Character codes for the escape matches and their replacements.
cdef enum:
    _AMP   = 0x26   # &
    _LT    = 0x3C   # <
    _GT    = 0x3E   # >
    _QUOT  = 0x22   # "
    _APOS  = 0x27   # '
    _SLASH = 0x2F   # /
    _EQ    = 0x3D   # =
    _DEL   = 0x7F
    _A     = 0x61   # a
    _G     = 0x67   # g
    _L     = 0x6C   # l
    _M     = 0x6D   # m
    _O     = 0x6F   # o
    _P     = 0x70   # p
    _Q     = 0x71   # q
    _T     = 0x74   # t
    _U     = 0x75   # u
    _SEMI  = 0x3B   # ;


# Runtime ABI version; must match `RUNTIME_ABI_VERSION` in src/compiler/opcodes.rs.
from typing import Final
VERSION: Final[int] = 1


class PythonJSXError(Exception):
    """Base class for errors raised by the PythonJSX runtime."""


class PythonJSXValueError(PythonJSXError, ValueError):
    """PythonJSX-specific `ValueError`. Subclasses ValueError so existing
    `except ValueError:` handlers still catch it."""


class PythonJSXTypeError(PythonJSXError, TypeError):
    """PythonJSX-specific `TypeError`."""


def assert_version(expected):
    """ABI guard called once at the top of every compiler-generated .py."""
    if expected != VERSION:
        raise PythonJSXError(
            f"pythonjsx: compiler/runtime ABI mismatch "
            f"(compiler emitted v{expected}, runtime is v{VERSION}). "
        )


# Pre-interned attr-render fragments, held as `cdef str` so the references
# in pjr_render_attr / _pjr_unpack_attrs are direct slot loads.
cdef str _SP        = ' '
cdef str _EQ_QUOT   = '="'
cdef str _CLOSE_Q   = '"'


include "escape.pxi"
include "template.pxi"
