"""Build the Cython runtime extension as part of `pip install`.

Metadata and the Rust binaries live in `pyproject.toml`; this file
exists only to declare the Cython extension, which `[tool.setuptools]`
cannot express directly. The Makefile builds the same extension
out-of-tree for local development.
"""

from setuptools import Extension, setup
from Cython.Build import cythonize

extensions = [
    Extension(
        "pythonjsx._native_cy",
        sources=["runtime-cy/_native_cy.pyx"],
        extra_compile_args=[
            "-O3",
            "-g",
            "-Wno-unreachable-code",
            "-Wno-deprecated-declarations",
        ],
    ),
]

setup(ext_modules=cythonize(extensions, language_level=3))
