"""Runtime shim for PythonJSX compiled code.

Exports the runtime primitives from the native Cython extension.
"""

from __future__ import annotations

from pythonjsx._native_cy import (
    JSXResult,
    JSXTemplate,
    PythonJSXError,
    PythonJSXTypeError,
    PythonJSXValueError,
    SafeStr,
    SLOT_SPREAD,
    SLOT_VALUE,
    SlotAttr,
    VERSION,
    assert_version,
)

__all__ = [
    "JSXResult",
    "JSXTemplate",
    "PythonJSXError",
    "PythonJSXTypeError",
    "PythonJSXValueError",
    "SafeStr",
    "SLOT_SPREAD",
    "SLOT_VALUE",
    "SlotAttr",
    "VERSION",
    "assert_version",
]
