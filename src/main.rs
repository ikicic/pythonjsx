//! PythonJSX compiler - compile .px files to .py

use clap::{CommandFactory, Parser};

use pythonjsx::cli::{self, Cli, Commands};

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Some(Commands::Compile {
            input,
            output,
            print_ast,
        }) => cli::compile_file(&input, output, print_ast),
        None => {
            let mut cmd = Cli::command();
            cmd.print_help().unwrap();
            1
        }
    };
    std::process::exit(exit_code);
}
