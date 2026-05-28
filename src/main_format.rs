//! Standalone PythonJSX JSX formatter.

use clap::Parser;
use pythonjsx::formatter::{format_source, FormatSettings};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "pythonjsx-format")]
#[command(about = "Format JSX portions of PythonJSX source")]
struct Cli {
    /// Input .px file or '-' for stdin
    input: String,

    /// Write formatted source to this file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Rewrite the input file in place
    #[arg(short = 'i', long)]
    in_place: bool,

    /// Exit nonzero if formatting would change the file
    #[arg(long)]
    check: bool,

    /// Maximum formatted line width
    #[arg(long, default_value_t = 100)]
    line_width: usize,

    /// Number of spaces per JSX indentation level
    #[arg(long, default_value_t = 4)]
    indent_width: usize,

    /// Allow simple existing multiline JSX to collapse when it fits
    #[arg(long)]
    collapse_multiline: bool,
}

fn main() {
    let cli = Cli::parse();
    std::process::exit(run(cli));
}

fn run(cli: Cli) -> i32 {
    if cli.in_place && cli.input == "-" {
        eprintln!("Error: --in-place cannot be used with stdin");
        return 2;
    }
    if cli.in_place && cli.output.is_some() {
        eprintln!("Error: --in-place and --output are mutually exclusive");
        return 2;
    }
    if cli.check && (cli.in_place || cli.output.is_some()) {
        eprintln!("Error: --check cannot be combined with --in-place or --output");
        return 2;
    }

    let source = if cli.input == "-" {
        match std::io::read_to_string(std::io::stdin()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                return 1;
            }
        }
    } else {
        match std::fs::read_to_string(&cli.input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", cli.input, e);
                return 1;
            }
        }
    };

    let settings = FormatSettings {
        line_width: cli.line_width,
        indent_width: cli.indent_width,
        preserve_multiline: !cli.collapse_multiline,
    };
    let formatted = match format_source(&source, &settings) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error formatting {}: {}", display_input(&cli.input), e);
            return 1;
        }
    };

    if cli.check {
        if formatted == source {
            0
        } else {
            eprintln!("{} would be reformatted", display_input(&cli.input));
            1
        }
    } else if cli.in_place {
        match std::fs::write(&cli.input, formatted) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("Error writing {}: {}", cli.input, e);
                1
            }
        }
    } else if let Some(output) = cli.output {
        match std::fs::write(&output, formatted) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("Error writing {}: {}", output.display(), e);
                1
            }
        }
    } else {
        print!("{}", formatted);
        0
    }
}

fn display_input(input: &str) -> &str {
    if input == "-" {
        "<stdin>"
    } else {
        input
    }
}
