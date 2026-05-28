//! PythonJSX Language Server - Language Server for .px files

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut server = pythonjsx::lsp::server::LspServer::new();
    server.run(std::io::BufReader::new(stdin), stdout);
}
