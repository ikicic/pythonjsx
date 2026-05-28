use std::path::PathBuf;

fn main() {
    let grammar_src = PathBuf::from("grammar/src");
    let parser_c = grammar_src.join("parser.c");
    let scanner_c = grammar_src.join("scanner.c");

    if !parser_c.exists() || !scanner_c.exists() {
        panic!(
            "Grammar not built. Run: make build-grammar\n\
             This requires: npm install && make -C grammar"
        );
    }

    let mut build = cc::Build::new();
    build
        .flag("-std=c11")
        .flag("-Wno-unused-but-set-variable")
        .flag("-Wno-unused-value")
        .include(&grammar_src)
        .file(&parser_c)
        .file(&scanner_c)
        .compile("tree-sitter-pythonjsx");
}
