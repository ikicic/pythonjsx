.PHONY: help venv test clean build test-py312 test-py313 test-py314 test-all \
        benchmark benchmark-compile benchmark-runtime \
        benchmark-pythonjsx-hot \
        runtime-cython runtime-cython-debug \
        profile-cython-annotate \
        compile-commands

.DEFAULT_GOAL := build

# Python for build + tests; test-pyXYZ targets override this.
PYTHON ?= ./venv/bin/python
# Lazy `=` so it resolves after `make venv`.
PY_SOABI = $(shell $(PYTHON) -c 'import sysconfig; print(sysconfig.get_config_var("EXT_SUFFIX"))')

help:
	@echo "Available targets:"
	@echo "  build            - Build Rust compiler + Cython runtime"
	@echo "  runtime-cython   - Build the Cython runtime → pythonjsx/_native_cy.<soabi>.so"
	@echo "  runtime-cython-debug"
	@echo "                   - Cython runtime with PJR_DEBUG=1 (invariant checks)"
	@echo "  test             - Run Python + Rust tests against the main venv"
	@echo "  test-py312       - Build the Cython runtime for Python 3.12 and run tests"
	@echo "  test-py313       - Same for Python 3.13"
	@echo "  test-py314       - Same for Python 3.14"
	@echo "  test-all         - Run test-py312, test-py313, test-py314, and cargo test"
	@echo "  benchmark        - Run compile + runtime benchmarks"
	@echo "  benchmark-pythonjsx-hot"
	@echo "                   - Run PythonJSX benchmark via sudo renice"
	@echo "  profile-cython-annotate"
	@echo "                   - Generate Cython -a HTML heatmap"
	@echo "  compile-commands - Generate compile_commands.json for clangd/ccls"
	@echo "  clean            - Remove build artifacts"

venv:
	@if [ ! -x "$(PYTHON)" ]; then uv venv venv --python=3.14; fi
	uv pip install --python $(PYTHON) -r requirements.txt
	uv pip install --python $(PYTHON) -r requirements-dev.txt

node_modules:
	@echo "Installing Node.js dependencies..."
	npm install

build: venv runtime-cython
	cargo build --release

# --- runtime extension ----------------------------------------------------

runtime-cython: pythonjsx/_native_cy$(PY_SOABI)

# Build the Cython extension for a given interpreter; SOABI-suffixed .so
# lets multi-version builds coexist.
define cython_compile
	$(1) -m cython -3 runtime-cy/_native_cy.pyx
	$(1) -c "import sysconfig, subprocess, sys; \
	    inc = sysconfig.get_path('include'); \
	    sys.exit(subprocess.call([ \
	        'cc', '-shared', '-fPIC', '-O3', '-g', '-Wno-unreachable-code', \
	        '-Wno-deprecated-declarations', \
	        '-I', inc, \
	        'runtime-cy/_native_cy.c', \
	        '-o', 'pythonjsx/_native_cy$(2)' \
	    ]))"
endef

pythonjsx/_native_cy$(PY_SOABI): runtime-cy/_native_cy.pyx $(wildcard runtime-cy/*.pxi)
	$(call cython_compile,$(PYTHON),$(PY_SOABI))

# Debug build: -DPJR_DEBUG=1 enables internal invariant checks.
runtime-cython-debug:
	$(PYTHON) -m cython -3 runtime-cy/_native_cy.pyx
	$(PYTHON) -c "import sysconfig, subprocess, sys; \
	    inc = sysconfig.get_path('include'); \
	    sys.exit(subprocess.call([ \
	        'cc', '-shared', '-fPIC', '-O0', '-g', '-DPJR_DEBUG=1', \
	        '-Wno-unreachable-code', \
	        '-Wno-deprecated-declarations', \
	        '-I', inc, \
	        'runtime-cy/_native_cy.c', \
	        '-o', 'pythonjsx/_native_cy$(PY_SOABI)' \
	    ]))"

test:
	$(PYTHON) -m unittest discover -s tests/python
	cargo test

# --- multi-version testing ------------------------------------------------

# Per-Python venv + build + tests.  SOABI-suffixed .sos coexist; main
# venv is untouched.  Can't reuse cython_compile: SOABI must be resolved
# after the venv is created, so the build is inlined.
define test_pyversion
	uv venv --python=$(1) venv-py$(1)
	uv pip install --python venv-py$(1)/bin/python -r requirements-dev.txt
	./venv-py$(1)/bin/python -m cython -3 runtime-cy/_native_cy.pyx
	./venv-py$(1)/bin/python -c "import sysconfig, subprocess, sys; \
	    inc = sysconfig.get_path('include'); \
	    soabi = sysconfig.get_config_var('EXT_SUFFIX'); \
	    sys.exit(subprocess.call([ \
	        'cc', '-shared', '-fPIC', '-O3', '-g', '-Wno-unreachable-code', \
	        '-Wno-deprecated-declarations', \
	        '-I', inc, 'runtime-cy/_native_cy.c', \
	        '-o', 'pythonjsx/_native_cy' + soabi \
	    ]))"
	./venv-py$(1)/bin/python -m unittest discover -s tests/python
endef

test-py312:
	$(call test_pyversion,3.12)

test-py313:
	$(call test_pyversion,3.13)

test-py314:
	$(call test_pyversion,3.14)

test-all: test-py312 test-py313 test-py314
	cargo test

benchmark: benchmark-compile benchmark-runtime
	@

benchmark-compile: build
	@echo "=== Compilation benchmark ==="
	$(PYTHON) benchmarks/bench_compile.py

benchmark-runtime: build
	@echo "=== Rendering benchmark: PythonJSX ==="
	$(PYTHON) benchmarks/bench_pythonjsx.py
	@echo
	@echo "=== Rendering benchmark: Jinja2 ==="
	$(PYTHON) benchmarks/bench_jinja2.py
	@echo
	@echo "=== Rendering benchmark: Django ==="
	$(PYTHON) benchmarks/bench_django.py

benchmark-pythonjsx-hot: build
	@echo "=== Rendering benchmark: PythonJSX (renice ) ==="
	@sudo -v
	@$(PYTHON) benchmarks/bench_pythonjsx.py & \
		PID=$$!; \
		sudo renice -n -20 $$PID >/dev/null; \
		wait $$PID

# --- profiling ------------------------------------------------------------

# Cython -a HTML: line-by-line heatmap of Python C-API interaction.
# Useful for spotting silent Python fallbacks in Cython code.
profile-cython-annotate: runtime-cy/_native_cy.html
	@echo "Open runtime-cy/_native_cy.html in a browser"

runtime-cy/_native_cy.html: runtime-cy/_native_cy.pyx
	$(PYTHON) -m cython -3 -a runtime-cy/_native_cy.pyx

# --- compile_commands.json ------------------------------------------------

# compile_commands.json so clangd/ccls can navigate the Cython-generated C
# (which pulls in Python headers discovered via sysconfig).
compile-commands: compile_commands.json

compile_commands.json: runtime-cy/_native_cy.c
	$(PYTHON) -c "import json, os, sysconfig; \
	    root = os.path.abspath('.'); \
	    inc = sysconfig.get_path('include'); \
	    entry = { \
	        'directory': root, \
	        'arguments': [ \
	            'cc', '-fPIC', '-O3', '-g', '-Wno-unreachable-code', \
	            '-I', inc, '-c', 'runtime-cy/_native_cy.c' \
	        ], \
	        'file': os.path.join(root, 'runtime-cy/_native_cy.c'), \
	    }; \
	    open('compile_commands.json','w').write(json.dumps([entry], indent=2))"
	@echo "Wrote compile_commands.json (regenerate after editing .pyx)"

# .c is a cythonize byproduct; regenerated when the .pyx changes.
runtime-cy/_native_cy.c: runtime-cy/_native_cy.pyx
	$(PYTHON) -m cython -3 runtime-cy/_native_cy.pyx

clean:
	cargo clean
	make -C grammar clean
	rm -rf node_modules
	rm -f pythonjsx/_native_cy*.so
	rm -f runtime-cy/_native_cy.c runtime-cy/_native_cy*.so runtime-cy/_native_cy.html
	rm -rf runtime-cy/build
	rm -f compile_commands.json
	rm -rf venv-py3.12 venv-py3.13 venv-py3.14
	find . -type d -name __pycache__ -exec rm -r {} +
	find . -type f -name "*.pyc" -delete
	find . -type f -name "*.pyo" -delete
