use std::collections::HashMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use crossbeam_channel::{unbounded, Sender};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::compiler::error::{CompileError, CompileErrorSeverity};
use crate::compiler::sourcemap::LineColumnMap;
use crate::compiler::Compiler;
use crate::lsp::mapping::{path_to_uri, uri_to_path, DocumentMapping};
use crate::lsp::stream::JsonRpcStream;
use crate::lsp::types::*;

pub struct LspServer {
    next_id: i64,
    workspace_root: Option<PathBuf>,
    temp_dir: Option<TempDir>,
    temp_root: Option<PathBuf>,
    documents_by_py_uri: HashMap<String, Arc<DocumentMapping>>,
    documents_by_original_uri: HashMap<String, Arc<DocumentMapping>>,
    compile_errors: HashMap<String, Vec<CompileError>>, // original_uri -> errors
    pyright_diagnostics: HashMap<String, Vec<Value>>,  // original_uri -> mapped diagnostics from Pyright
    compiler: Compiler,
    pyright_process: Option<Child>,
    pyright_sender: Option<Sender<Value>>,
    pending_requests: HashMap<String, PendingRequest>, // id -> request info
}

struct PendingRequest {
    method: String,
    client_id: Option<Value>,
    origin_mapping: Option<Arc<DocumentMapping>>,
}

enum Message {
    Client(Value),
    Pyright(Value),
    Exit,
}

impl LspServer {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            workspace_root: None,
            temp_dir: None,
            temp_root: None,
            documents_by_py_uri: HashMap::new(),
            documents_by_original_uri: HashMap::new(),
            compile_errors: HashMap::new(),
            pyright_diagnostics: HashMap::new(),
            compiler: Compiler::new(None),
            pyright_process: None,
            pyright_sender: None,
            pending_requests: HashMap::new(),
        }
    }

    pub fn run<R: std::io::BufRead + Send + 'static, W: std::io::Write + Send + 'static>(
        &mut self,
        reader: R,
        writer: W,
    ) {
        let (tx, rx) = unbounded();
        let tx_client = tx.clone();

        // Client reader thread
        thread::spawn(move || {
            let mut stream = JsonRpcStream::new(reader, std::io::sink()); // We only read here
            loop {
                match stream.read_message() {
                    Ok(Some(msg)) => {
                        if tx_client.send(Message::Client(msg)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = tx_client.send(Message::Exit);
                        break; // EOF
                    }
                    Err(e) => {
                        eprintln!("Error reading from client: {}", e);
                        let _ = tx_client.send(Message::Exit);
                        break;
                    }
                }
            }
        });

        let mut client_writer = JsonRpcStream::new(std::io::empty(), writer); // We only write here

        loop {
            match rx.recv() {
                Ok(Message::Client(msg)) => {
                    if self.handle_client_message(&mut client_writer, msg, &tx) {
                        break;
                    }
                }
                Ok(Message::Pyright(msg)) => self.handle_pyright_message(&mut client_writer, msg),
                Ok(Message::Exit) => break,
                Err(_) => break,
            }
        }
    }

    fn handle_client_message<W: std::io::Write>(
        &mut self,
        writer: &mut JsonRpcStream<std::io::Empty, W>,
        msg: Value,
        tx: &Sender<Message>,
    ) -> bool {
        if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
            if msg.get("id").is_some() {
                // Request
                match method {
                    "initialize" => self.handle_initialize(writer, msg, tx),
                    "shutdown" => self.handle_shutdown(writer, msg),
                    "textDocument/definition" => self.handle_definition(msg),
                    _ => self.forward_request(msg),
                }
            } else {
                // Notification
                match method {
                    "initialized" => self.forward_notification(msg),
                    "textDocument/didOpen" => self.handle_did_open(writer, msg),
                    "textDocument/didChange" => self.handle_did_change(writer, msg),
                    "textDocument/didClose" => self.handle_did_close(msg),
                    "workspace/didChangeConfiguration" => self.forward_notification(msg),
                    "textDocument/didSave" => self.forward_notification(msg),
                    "exit" => {
                        self.forward_notification(msg);
                        return true;
                    }
                    _ => self.forward_notification(msg),
                }
            }
        }
        false
    }

    fn handle_initialize<W: std::io::Write>(
        &mut self,
        writer: &mut JsonRpcStream<std::io::Empty, W>,
        msg: Value,
        tx: &Sender<Message>,
    ) {
        let params: InitializeParams = match msg.get("params").and_then(|p| serde_json::from_value(p.clone()).ok()) {
            Some(p) => p,
            None => {
                eprintln!("Invalid initialize params");
                return;
            }
        };

        if let Some(uri) = &params.root_uri {
            self.workspace_root = uri_to_path(uri);
        } else if let Some(path) = &params.root_path {
            self.workspace_root = Some(PathBuf::from(path));
        } else {
            self.workspace_root = std::env::current_dir().ok();
        }

        let temp_parent = self.workspace_root.clone().unwrap_or_else(|| std::env::current_dir().unwrap());
        let temp_dir = TempDir::new_in(&temp_parent).expect("Failed to create temp dir");
        self.temp_root = Some(temp_dir.path().to_path_buf());
        self.temp_dir = Some(temp_dir);

        self.start_pyright(tx.clone());

        // Forward initialize to pyright
        let mut pyright_params = params.clone();
        if let Some(root) = &self.workspace_root {
            pyright_params.root_uri = Some(path_to_uri(root));
            pyright_params.root_path = Some(root.to_string_lossy().to_string());
            pyright_params.workspace_folders = Some(vec![WorkspaceFolder {
                uri: path_to_uri(root),
                name: root.file_name().unwrap_or_default().to_string_lossy().to_string(),
            }]);
        }

        let result = InitializeResult {
            capabilities: json!({
                "textDocumentSync": {
                    "openClose": true,
                    "change": 1 // Full sync
                },
                "definitionProvider": true,
            }),
            server_info: Some(ServerInfo {
                name: "pythonjsx-langserver".to_string(),
                version: Some("0.1.0".to_string()),
            }),
        };

        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: msg.get("id").cloned(),
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        };

        writer.send_message(&serde_json::to_value(response).unwrap()).unwrap();

        // Send initialize to pyright
        self.send_request_pyright("initialize", serde_json::to_value(pyright_params).unwrap(), None, None);

        // Send configuration
        self.send_pyright_configuration();
    }

    fn start_pyright(&mut self, tx: Sender<Message>) {
        let cmd = std::env::var("PYTHONJSX_PYRIGHT_CMD").unwrap_or_else(|_| "basedpyright-langserver".to_string());
        let mut args = shell_words::split(&cmd).unwrap_or_else(|_| vec![cmd.clone()]);
        if !args.contains(&"--stdio".to_string()) {
            args.push("--stdio".to_string());
        }

        let program = args.remove(0);
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Failed to start pyright");

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let (sender_tx, sender_rx) = unbounded::<Value>();
        self.pyright_sender = Some(sender_tx);
        self.pyright_process = Some(child);

        // Pyright writer thread
        thread::spawn(move || {
            let mut stream = JsonRpcStream::new(std::io::empty(), stdin);
            while let Ok(msg) = sender_rx.recv() {
                if stream.send_message(&msg).is_err() {
                    break;
                }
            }
        });

        // Pyright reader thread
        thread::spawn(move || {
            let mut stream = JsonRpcStream::new(BufReader::new(stdout), std::io::sink());
            loop {
                match stream.read_message() {
                    Ok(Some(msg)) => {
                        if tx.send(Message::Pyright(msg)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("Error reading from pyright: {}", e);
                        break;
                    }
                }
            }
        });
    }

    // FIXME: shutdown is forwarded to pyright but we never wait for the
    // response or reap the child — pyright can linger until editor exit.
    // Proper fix: join reader/writer threads, await pyright's `shutdown`
    // response, send `exit`, then `child.wait()`.
    fn handle_shutdown<W: std::io::Write>(&mut self, writer: &mut JsonRpcStream<std::io::Empty, W>, msg: Value) {
        self.send_request_pyright("shutdown", json!({}), None, None);
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: msg.get("id").cloned(),
            result: Some(Value::Null),
            error: None,
        };
        writer.send_message(&serde_json::to_value(response).unwrap()).unwrap();
    }

    fn handle_did_open<W: std::io::Write>(
        &mut self,
        writer: &mut JsonRpcStream<std::io::Empty, W>,
        msg: Value,
    ) {
        let params: DidOpenTextDocumentParams = match msg.get("params").and_then(|p| serde_json::from_value(p.clone()).ok()) {
            Some(p) => p,
            None => {
                eprintln!("Invalid didOpen params");
                return;
            }
        };
        let uri = params.text_document.uri;
        let text = params.text_document.text;

        let language = if uri.ends_with(".px") { "px" } else { "py" };

        if let Some((mapping, errors)) = self.build_mapping(&uri, &text, language) {
            let mapping = Arc::new(mapping);
            self.documents_by_original_uri.insert(uri.clone(), mapping.clone());
            self.documents_by_py_uri.insert(mapping.py_uri.clone(), mapping.clone());

            self.compile_errors.insert(uri.clone(), errors.clone());
            self.send_pyright_did_open(&mapping, Some(params.text_document.version));
            if !errors.is_empty() {
                self.publish_compile_diagnostics(writer, &uri, &text, &errors, Some(params.text_document.version));
            }
        }
    }

    fn handle_did_change<W: std::io::Write>(
        &mut self,
        writer: &mut JsonRpcStream<std::io::Empty, W>,
        msg: Value,
    ) {
        let params: DidChangeTextDocumentParams = match msg.get("params").and_then(|p| serde_json::from_value(p.clone()).ok()) {
            Some(p) => p,
            None => {
                eprintln!("Invalid didChange params");
                return;
            }
        };
        let uri = params.text_document.uri;

        if self.documents_by_original_uri.contains_key(&uri) {
            // We assume full sync for now as per capabilities
            if let Some(change) = params.content_changes.first() {
                let new_text = &change.text;
                let language = if uri.ends_with(".px") { "px" } else { "py" };

                if let Some((new_mapping, errors)) = self.build_mapping(&uri, new_text, language) {
                    let new_mapping = Arc::new(new_mapping);
                    self.documents_by_original_uri.insert(uri.clone(), new_mapping.clone());
                    self.documents_by_py_uri.insert(new_mapping.py_uri.clone(), new_mapping.clone());

                    self.compile_errors.insert(uri.clone(), errors.clone());
                    self.send_pyright_did_change(&new_mapping, Some(params.text_document.version));
                    if !errors.is_empty() {
                        self.publish_compile_diagnostics(writer, &uri, new_text, &errors, Some(params.text_document.version));
                    }
                }
            }
        }
    }

    fn handle_did_close(&mut self, msg: Value) {
        let params: DidCloseTextDocumentParams = match msg.get("params").and_then(|p| serde_json::from_value(p.clone()).ok()) {
            Some(p) => p,
            None => {
                eprintln!("Invalid didClose params");
                return;
            }
        };
        let uri = params.text_document.uri.clone();

        if let Some(mapping) = self.documents_by_original_uri.remove(&uri) {
            self.documents_by_py_uri.remove(&mapping.py_uri);
            self.compile_errors.remove(&uri);
            self.pyright_diagnostics.remove(&uri);

            // Forward close with py uri
            let mut close_params = params.clone();
            close_params.text_document.uri = mapping.py_uri.clone();

            self.forward_notification(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didClose",
                "params": close_params
            }));

            // Delete temp file
            if let Ok(path) = uri_to_path(&mapping.py_uri).ok_or("invalid uri") {
                 let _ = std::fs::remove_file(path);
            }
        }
    }

    fn build_mapping(&self, uri: &str, text: &str, language: &str) -> Option<(DocumentMapping, Vec<CompileError>)> {
        // TODO: per-keystroke `std::fs::write` is a latency hotspot.
        // Switching to an overlay-only flow (didChange) skips the disk
        // round-trip; pair with Incremental sync for a larger win.
        let original_path = uri_to_path(uri)?;
        let py_path = self.temp_path_for(&original_path, language)?;

        let (py_text, source_map, errors) = if language == "px" {
            match self.compiler.compile(text) {
                Ok((code, map, errs)) => (code, Some(map), errs),
                Err(e) => {
                    eprintln!("Compile error: {}", e);
                    return None;
                }
            }
        } else {
            (text.to_string(), None, vec![])
        };

        if let Some(parent) = py_path.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        std::fs::write(&py_path, &py_text).ok()?;

        let py_uri = path_to_uri(&py_path);

        let mapping = DocumentMapping::new(
            uri.to_string(),
            py_uri,
            source_map,
            text.to_string(),
            py_text,
        );
        Some((mapping, errors))
    }

    fn compile_errors_to_diagnostics(text: &str, errors: &[CompileError]) -> Vec<Value> {
        if errors.is_empty() {
            return vec![];
        }
        let line_map = LineColumnMap::new(text);
        let mut diags = Vec::with_capacity(errors.len());
        for err in errors {
            let (start_line, start_col) = line_map.byte_to_line_col(err.range.start);
            let (end_line, end_col) = line_map.byte_to_line_col(err.range.end);
            let severity = match err.severity {
                CompileErrorSeverity::Error => 1,
                CompileErrorSeverity::Warning => 2,
            };
            diags.push(json!({
                "range": {
                    "start": { "line": start_line, "character": start_col },
                    "end": { "line": end_line, "character": end_col }
                },
                "severity": severity,
                "message": err.message,
                "source": "pythonjsx"
            }));
        }
        diags
    }

    fn publish_compile_diagnostics<W: std::io::Write>(
        &self,
        writer: &mut JsonRpcStream<std::io::Empty, W>,
        uri: &str,
        text: &str,
        errors: &[CompileError],
        version: Option<i32>,
    ) {
        let compile_diags = Self::compile_errors_to_diagnostics(text, errors);
        let pyright_diags = self.pyright_diagnostics.get(uri).cloned().unwrap_or_default();
        let mut all_diags = compile_diags;
        all_diags.extend(pyright_diags);

        let params = json!({
            "uri": uri,
            "version": version,
            "diagnostics": all_diags
        });
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": params
        });
        let _ = writer.send_message(&notif);
    }

    fn temp_path_for(&self, original_path: &Path, language: &str) -> Option<PathBuf> {
        let temp_root = self.temp_root.as_ref()?;
        let workspace_root = self.workspace_root.as_ref()?;

        let relative = original_path.strip_prefix(workspace_root).unwrap_or_else(|_| {
             // Fallback: use filename if not in workspace
             Path::new(original_path.file_name().unwrap_or_default())
        });

        let mut temp_path = temp_root.join(relative);
        if language == "px" {
            temp_path.set_extension("py");
        }
        Some(temp_path)
    }

    fn send_pyright_did_open(&self, mapping: &DocumentMapping, version: Option<i32>) {
        let params = json!({
            "textDocument": {
                "uri": mapping.py_uri,
                "languageId": "python",
                "version": version,
                "text": mapping.py_text
            }
        });
        self.send_notification_pyright("textDocument/didOpen", params);
    }

    fn send_pyright_did_change(&self, mapping: &DocumentMapping, version: Option<i32>) {
        let params = json!({
            "textDocument": {
                "uri": mapping.py_uri,
                "version": version
            },
            "contentChanges": [
                {
                    "text": mapping.py_text
                }
            ]
        });
        self.send_notification_pyright("textDocument/didChange", params);
    }

    fn send_pyright_configuration(&self) {
        let settings = json!({
            "python": { "analysis": { "diagnosticMode": "openFilesOnly", "typeCheckingMode": "basic" } },
            "basedpyright": { "analysis": { "diagnosticMode": "openFilesOnly", "typeCheckingMode": "basic" } }
        });
        let params = json!({ "settings": settings });
        self.send_notification_pyright("workspace/didChangeConfiguration", params);
    }

    fn handle_definition(&mut self, msg: Value) {
        let mut params: Value = msg.get("params").unwrap().clone();
        let uri = params.get("textDocument").unwrap().get("uri").unwrap().as_str().unwrap();

        let mapping = self.documents_by_original_uri.get(uri).cloned();

        if let Some(mapping) = &mapping {
            // Rewrite URI
            params["textDocument"]["uri"] = json!(mapping.py_uri);

            // Map position
            let pos: Position = serde_json::from_value(params["position"].clone()).unwrap();
            if let Some(py_pos) = mapping.map_px_position_to_py_position(&pos) {
                params["position"] = serde_json::to_value(py_pos).unwrap();
            }
        }

        self.send_request_pyright("textDocument/definition", params, msg.get("id").cloned(), mapping);
    }

    // TODO: response remapping is only wired up for `definition` and
    // diagnostics.  Other LSP requests (hover, completion, references, …)
    // forward URIs into py-space but return ranges/locations pointing at
    // the temp `.py`.  Longer term: a generic walker that rewrites every
    // Location/Range/uri field in any JSON response.
    fn forward_request(&mut self, msg: Value) {
        let method = msg.get("method").unwrap().as_str().unwrap();
        let mut params = msg.get("params").cloned().unwrap_or(json!({}));
        let id = msg.get("id").cloned();

        // Rewrite URI if present
        if let Some(uri) = params.get("textDocument").and_then(|td| td.get("uri")).and_then(|u| u.as_str()) {
            if let Some(mapping) = self.documents_by_original_uri.get(uri) {
                params["textDocument"]["uri"] = json!(mapping.py_uri);
            }
        }

        self.send_request_pyright(method, params, id, None);
    }

    fn forward_notification(&mut self, msg: Value) {
        let method = msg.get("method").unwrap().as_str().unwrap();
        let mut params = msg.get("params").cloned().unwrap_or(json!({}));

        // Rewrite URI if present
        if let Some(uri) = params.get("textDocument").and_then(|td| td.get("uri")).and_then(|u| u.as_str()) {
            if let Some(mapping) = self.documents_by_original_uri.get(uri) {
                params["textDocument"]["uri"] = json!(mapping.py_uri);
            }
        }

        self.send_notification_pyright(method, params);
    }

    fn send_request_pyright(&mut self, method: &str, params: Value, client_id: Option<Value>, origin_mapping: Option<Arc<DocumentMapping>>) {
        if let Some(sender) = &self.pyright_sender {
            let id = self.next_id;
            self.next_id += 1;

            let req = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            });

            self.pending_requests.insert(id.to_string(), PendingRequest {
                method: method.to_string(),
                client_id,
                origin_mapping,
            });

            let _ = sender.send(req);
        }
    }

    fn send_notification_pyright(&self, method: &str, params: Value) {
        if let Some(sender) = &self.pyright_sender {
            let notif = json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params
            });
            let _ = sender.send(notif);
        }
    }

    fn handle_pyright_message<W: std::io::Write>(&mut self, client_writer: &mut JsonRpcStream<std::io::Empty, W>, msg: Value) {
        if msg.get("id").is_some() && msg.get("method").is_none() {
            // Response
            self.handle_pyright_response(client_writer, msg);
        } else if msg.get("method").is_some() {
            // Notification or Request from server
            if msg.get("id").is_some() {
                // Request from server (e.g. workspace/configuration)
                self.handle_pyright_request(msg);
            } else {
                // Notification
                self.handle_pyright_notification(client_writer, msg);
            }
        }
    }

    fn handle_pyright_response<W: std::io::Write>(&mut self, client_writer: &mut JsonRpcStream<std::io::Empty, W>, msg: Value) {
        let id_val = msg.get("id").unwrap();
        let id_str = match id_val {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => return,
        };

        if let Some(pending) = self.pending_requests.remove(&id_str) {
            if let Some(client_id) = pending.client_id {
                let mut response = msg.clone();
                response["id"] = client_id;

                if let Some(result) = response.get_mut("result") {
                    if pending.method == "textDocument/definition" {
                        // Map definition result
                        self.map_definition_result(result, pending.origin_mapping.as_deref());
                    }
                }

                let _ = client_writer.send_message(&response);
            }
        }
    }

    fn map_definition_result(&self, result: &mut Value, _origin_mapping: Option<&DocumentMapping>) {
        if let Some(arr) = result.as_array_mut() {
            for item in arr {
                self.map_location(item);
            }
        } else if result.is_object() {
            self.map_location(result);
        }
    }

    fn map_location(&self, item: &mut Value) {
        if let Some(uri) = item.get("uri").and_then(|u| u.as_str()) {
             if let Some(mapping) = self.documents_by_py_uri.get(uri) {
                 item["uri"] = json!(mapping.original_uri);
                 if let Some(range) = item.get("range") {
                     let range_obj: Range = serde_json::from_value(range.clone()).unwrap();
                     if let Some(mapped_range) = mapping.map_py_range_to_px_range(&range_obj) {
                         item["range"] = serde_json::to_value(mapped_range).unwrap();
                     }
                 }
             }
        }
        // Handle LocationLink (targetUri)
        if let Some(target_uri) = item.get("targetUri").and_then(|u| u.as_str()) {
             if let Some(mapping) = self.documents_by_py_uri.get(target_uri) {
                 item["targetUri"] = json!(mapping.original_uri);
                 if let Some(range) = item.get("targetRange") {
                     let range_obj: Range = serde_json::from_value(range.clone()).unwrap();
                     if let Some(mapped_range) = mapping.map_py_range_to_px_range(&range_obj) {
                         item["targetRange"] = serde_json::to_value(mapped_range).unwrap();
                     }
                 }
                 if let Some(range) = item.get("targetSelectionRange") {
                     let range_obj: Range = serde_json::from_value(range.clone()).unwrap();
                     if let Some(mapped_range) = mapping.map_py_range_to_px_range(&range_obj) {
                         item["targetSelectionRange"] = serde_json::to_value(mapped_range).unwrap();
                     }
                 }
             }
        }
    }

    fn handle_pyright_notification<W: std::io::Write>(&mut self, client_writer: &mut JsonRpcStream<std::io::Empty, W>, msg: Value) {
        let method = msg.get("method").unwrap().as_str().unwrap();
        if method == "textDocument/publishDiagnostics" {
            let mut params = msg.get("params").unwrap().clone();
            let uri = params.get("uri").unwrap().as_str().unwrap();

            if let Some(mapping) = self.documents_by_py_uri.get(uri) {
                let original_uri = &mapping.original_uri;
                params["uri"] = json!(original_uri);

                let mut mapped_diagnostics = Vec::new();
                if let Some(diagnostics) = params.get("diagnostics").and_then(|d| d.as_array()) {
                    for diag in diagnostics.iter() {
                        let mut mapped_diag = diag.clone();
                        let range: Range = serde_json::from_value(diag["range"].clone()).unwrap();

                        if let Some(mapped_range) = mapping.map_py_range_to_px_range(&range) {
                            mapped_diag["range"] = serde_json::to_value(mapped_range).unwrap();

                            // Map related information
                            if let Some(related) = mapped_diag.get_mut("relatedInformation").and_then(|r| r.as_array_mut()) {
                                for rel in related {
                                    if let Some(loc) = rel.get_mut("location") {
                                        self.map_location(loc);
                                    }
                                }
                            }

                            mapped_diagnostics.push(mapped_diag);
                        }
                    }
                }
                self.pyright_diagnostics.insert(original_uri.clone(), mapped_diagnostics.clone());

                // Merge with compile errors
                let compile_diags = self.compile_errors.get(original_uri).map(|errs| {
                    Self::compile_errors_to_diagnostics(&mapping.original_text, errs)
                }).unwrap_or_default();
                let mut all_diags = compile_diags;
                all_diags.extend(mapped_diagnostics);
                params["diagnostics"] = json!(all_diags);

                let notif = json!({
                    "jsonrpc": "2.0",
                    "method": method,
                    "params": params
                });
                let _ = client_writer.send_message(&notif);
            } else {
                // Pass through if not mapped
                let _ = client_writer.send_message(&msg);
            }
        } else {
            let _ = client_writer.send_message(&msg);
        }
    }

    fn handle_pyright_request(&mut self, msg: Value) {
        // Handle requests from pyright (like workspace/configuration)
        let method = msg.get("method").unwrap().as_str().unwrap();
        if method == "workspace/configuration" {
             // Respond with empty config or default
             let response = json!({
                 "jsonrpc": "2.0",
                 "id": msg.get("id"),
                 "result": [
                     { "analysis": { "diagnosticMode": "openFilesOnly", "typeCheckingMode": "basic" } }
                 ]
             });
             if let Some(sender) = &self.pyright_sender {
                 let _ = sender.send(response);
             }
        } else if method == "workspace/workspaceFolders" {
             let folders = if let Some(root) = &self.temp_root {
                 vec![json!({ "uri": path_to_uri(root), "name": root.file_name().unwrap().to_string_lossy() })]
             } else {
                 vec![]
             };
             let response = json!({
                 "jsonrpc": "2.0",
                 "id": msg.get("id"),
                 "result": folders
             });
             if let Some(sender) = &self.pyright_sender {
                 let _ = sender.send(response);
             }
        }
    }
}
