# PythonJSX

PythonJSX is an experimental Python+JSX compiler. It lets you write `.px`
files with JSX expressions and compile them to regular Python code.

It includes:

- a Rust compiler for `.px` files
- a Cython runtime for rendering JSX expressions into HTML
- a basic JSX formatter
- a basic language server for editor integration

## Quick Start

Install from GitHub:

```bash
pip install git+https://github.com/ikicic/pythonjsx.git
```

PythonJSX requires Python 3.12+ and a Rust toolchain when installing from source.

Create `hello.px`:

```python
def Header(title: str):
    return <header><h1>{title}</h1></header>

def App():
    return <main><Header title="Hello" /></main>

print(App().to_html())
```

Run it:

```bash
python -m pythonjsx run hello.px
```

Output:

```html
<main><header><h1>Hello</h1></header></main>
```

## Syntax

JSX is expression syntax.
You can assign it, return it, pass it to functions, print it, or nest it inside other Python expressions.

```python
print(<div id="main" class="page">Content</div>)
# <div id="main" class="page">Content</div>

href = "/docs"
title = "Docs"
print(<a class="link" href={href}>{title}</a>)
# <a class="link" href="/docs">Docs</a>

def Header(*children, title: str):
    return <header><h1>{title}</h1>{children}</header>

print(<Header title="Hello">Welcome</Header>)
# <header><h1>Hello</h1>Welcome</header>

def Component(*children, **attrs):
    return <section {**attrs}>{*children}</section>

props = {"class": "panel"}
children = [<p>One</p>, <p>Two</p>]
print(<Component {**props}>{*children}</Component>)
# <section class="panel"><p>One</p><p>Two</p></section>

print(<><span>A</span><span>B</span></>)
# <span>A</span><span>B</span>

print(<input type="checkbox" disabled checked={True} data-not-included={False} />)
# <input type="checkbox" disabled checked/>

items = ["One", "Two"]
print(<ul>{<li>{item}</li> for item in items}</ul>)
# <ul><li>One</li><li>Two</li></ul>
```

## Runtime And Escaping

JSX expressions return PythonJSX runtime values.
Render them with either `str(node)` or `node.to_html()`:

```python
node = <div>Hello, {name}</div>
html = node.to_html()
```

Use `to_html_document()` when rendering a complete document:

```python
def Page():
    return <html><body>Hello</body></html>

print(Page().to_html_document())
# <!DOCTYPE html>
# <html><body>Hello</body></html>
```

Text and attributes are escaped by default:

```python
print(<div>{'<script>'}</div>)
# <div>&lt;script&gt;</div>
```

Use `SafeStr` only for trusted HTML:

```python
from pythonjsx.runtime import SafeStr

print(<div>{SafeStr("<strong>trusted</strong>")}</div>)
# <div><strong>trusted</strong></div>
```

## Usage

### Run or Compile Files

```bash
python -m pythonjsx run file.px
python -m pythonjsx compile file.px
python -m pythonjsx compile file.px -o file.py
```

The installed Rust compiler binary can also be called directly:

```bash
pythonjsx compile file.px
```

### Format Files

```bash
python -m pythonjsx format file.px          # write formatted source to stdout
python -m pythonjsx format file.px -i       # rewrite file in place
python -m pythonjsx format file.px --check  # exit nonzero if formatting would change
```

### Import `.px` Files

To be able to import `.px` files using the `import` statement, install the import hook once during startup:

```python
import pythonjsx.importer

pythonjsx.importer.install()
```

### Use JSX in `.py` Files

To use JSX in `.py` files, add a coding cookie:

```python
# coding: pythonjsx

def Hello(name: str):
    return <div>Hello, {name}</div>
```

Register the codec once before importing those files:

```python
import pythonjsx.codec

pythonjsx.codec.register()
```

## Development

Prerequisites: [uv](https://docs.astral.sh/uv/), Python 3.14, Rust, and a C compiler with Python development headers.

```bash
make
make test
```

The full Rust test suite includes LSP integration tests that use `basedpyright`.

## Similar Projects

[pyjsx](https://github.com/tomasr8/pyjsx)
