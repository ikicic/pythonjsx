//! CLI for PythonJSX compiler.

use std::borrow::Cow;
use std::io::IsTerminal;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::compiler::sourcemap::LineColumnMap;
use crate::compiler::Compiler;
use crate::parser;
use crate::CompileErrorSeverity;

/// Top-level clap parser.  Lives in the library (not `main.rs`) so tests
/// can parse argv fixtures without shelling out.
#[derive(Parser)]
#[command(name = "pythonjsx")]
#[command(about = "PythonJSX compiler - compile .px files to .py")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Compile a .px file to .py
    Compile {
        /// Input .px file or '-' for stdin
        input: String,
        /// Output .py file (default: write compiled code to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Print the parse tree (AST) to stderr for debugging
        #[arg(long)]
        print_ast: bool,
    },
}

// ANSI escape sequences used for pretty-printing diagnostics.
const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_BOLD_RED: &str = "\x1b[1;31m";
const ANSI_BOLD_YELLOW: &str = "\x1b[1;33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_YELLOW: &str = "\x1b[33m";

/// Decide whether to emit ANSI color on stderr.  `NO_COLOR` off,
/// `CLICOLOR_FORCE` on, `TERM=dumb` off, otherwise tty-only.
fn use_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Ok(v) = std::env::var("CLICOLOR_FORCE") {
        if !v.is_empty() && v != "0" {
            return true;
        }
    }
    if matches!(std::env::var("TERM").as_deref(), Ok("dumb")) {
        return false;
    }
    std::io::stderr().is_terminal()
}

/// Wrap `s` in ANSI codes if enabled (Cow to skip alloc on no-color paths).
fn paint<'a>(enabled: bool, code: &str, s: &'a str) -> Cow<'a, str> {
    if enabled {
        Cow::Owned(format!("{}{}{}", code, s, ANSI_RESET))
    } else {
        Cow::Borrowed(s)
    }
}

pub fn compile_file(
    input_path: &str,
    output_path: Option<PathBuf>,
    print_ast: bool,
) -> i32 {
    let source = if input_path == "-" {
        match std::io::read_to_string(std::io::stdin()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                return 1;
            }
        }
    } else {
        match std::fs::read_to_string(input_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", input_path, e);
                return 1;
            }
        }
    };

    if print_ast {
        match parser::parse(&source) {
            Ok(tree) => parser::print_tree(&tree, &source),
            Err(e) => eprintln!("Parse error (cannot print AST): {}", e),
        }
    }

    let compiler = Compiler::new(None);
    let (compiled, _source_map, errors) = match compiler.compile(&source) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Error compiling: {}", e);
            return 1;
        }
    };

    let has_error = errors
        .iter()
        .any(|e| e.severity == CompileErrorSeverity::Error);
    if !errors.is_empty() {
        let lcm = LineColumnMap::new(&source);
        let display = if input_path == "-" { "<stdin>" } else { input_path };
        let source_lines: Vec<&str> = source.split('\n').collect();
        let color = use_color();
        for diag in &errors {
            let is_warning = diag.severity == CompileErrorSeverity::Warning;
            let (sl, sc) = lcm.byte_to_line_col(diag.range.start);
            let (el, ec) = lcm.byte_to_line_col(diag.range.end);

            // Header: `File "<path>", line <N>`. Temporaries are bound so
            // `paint` can borrow them.
            let path_text = format!("{:?}", display);
            let line_text = (sl + 1).to_string();
            let path_painted = paint(color, ANSI_CYAN, &path_text);
            let line_painted = paint(color, ANSI_YELLOW, &line_text);
            eprintln!("  File {}, line {}", path_painted, line_painted);

            // Underline the offending range on the start line (multi-line
            // ranges underline only up to end-of-line).
            if let Some(line) = source_lines.get(sl) {
                eprintln!("    {}", line);

                // Pad/underline width counted in chars (not bytes) so the `^`
                // aligns with the glyph in most fonts.
                let line_bytes = line.as_bytes();
                let prefix_bytes = sc.min(line.len());
                let prefix = std::str::from_utf8(&line_bytes[..prefix_bytes])
                    .unwrap_or("");
                let pad_chars = prefix.chars().count();
                let end_bytes = if sl == el { ec.min(line.len()) } else { line.len() };
                let span = std::str::from_utf8(
                    &line_bytes[prefix_bytes..end_bytes.max(prefix_bytes)],
                )
                .unwrap_or("");
                let caret_chars = span.chars().count().max(1);
                let mut pad = String::with_capacity(pad_chars);
                for _ in 0..pad_chars {
                    pad.push(' ');
                }
                let mut carets = String::with_capacity(caret_chars);
                for _ in 0..caret_chars {
                    carets.push('^');
                }
                let caret_color = if is_warning { ANSI_YELLOW } else { ANSI_RED };
                eprintln!("    {}{}", pad, paint(color, caret_color, &carets));
            }

            let (label, label_color) = if is_warning {
                ("PythonJSXWarning:", ANSI_BOLD_YELLOW)
            } else {
                ("PythonJSXError:", ANSI_BOLD_RED)
            };
            eprintln!(
                "{} {}",
                paint(color, label_color, label),
                paint(color, ANSI_BOLD, &diag.message)
            );
        }
    }

    // On error, emit nothing — half-compiled output would just trigger a
    // second wave of confusing errors when piped into `python`.
    if has_error {
        return 1;
    }

    match output_path {
        None => {
            print!("{}", compiled);
            0
        }
        Some(out_path) => match std::fs::write(&out_path, compiled) {
            Ok(_) => {
                eprintln!("Compiled {} -> {}", input_path, out_path.display());
                0
            }
            Err(e) => {
                eprintln!("Error writing {}: {}", out_path.display(), e);
                1
            }
        },
    }
}

