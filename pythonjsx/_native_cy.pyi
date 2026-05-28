"""Type stubs for the Cython native runtime (`_native_cy`).

The compiler emits `_tpl_N = JSXTemplate(...)` at module scope and
`_tpl_N(*args)` at call sites; each call returns a `JSXResult` that
renders on `.to_html()`.

`VERSION` / `assert_version` are consumed by the compiler's ABI-check
preamble emitted at the top of every generated module.
"""

from typing import Any, Final


class JSXTemplate:
    def __init__(self, *chunks: Any) -> None: ...
    def __call__(self, *args: Any) -> "JSXResult": ...
    @property
    def n_instrs(self) -> int: ...
    @property
    def n_slots(self) -> int: ...
    @property
    def static_bytes(self) -> int: ...


class JSXResult:
    def to_html(self) -> str: ...
    def to_html_document(self, prefix: str = ...) -> str: ...
    def __str__(self) -> str: ...


class SlotAttr:
    name: str
    def __init__(self, name: str) -> None: ...


class SafeStr:
    """Opaque wrapper marking a string as already HTML-safe.
    Rendered verbatim (no escaping) in content and attribute positions."""
    s: str
    def __init__(self, s: str) -> None: ...


SLOT_VALUE: Any   # opaque sentinel singleton
SLOT_SPREAD: Any  # opaque sentinel singleton


VERSION: Final[int]


def assert_version(expected: int) -> None: ...


class PythonJSXError(Exception): ...


class PythonJSXValueError(PythonJSXError, ValueError): ...


class PythonJSXTypeError(PythonJSXError, TypeError): ...
