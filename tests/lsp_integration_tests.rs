use std::io::{BufRead, BufReader, Write, Read};
use std::process::{Command, Stdio, Child, ChildStdin};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;
use serde_json::{json, Value};

struct TestLspServer {
    process: Child,
    stdin: Option<ChildStdin>,
    receiver: Receiver<Value>,
    next_id: i64,
}

impl TestLspServer {
    fn new() -> Self {
        // Assume the binary is built at target/debug/pythonjsx-langserver
        let bin_path = std::env::current_dir().unwrap().join("target/debug/pythonjsx-langserver");
        assert!(bin_path.exists(), "Binary not found at {:?}", bin_path);

        let mut process = Command::new(bin_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Failed to spawn pythonjsx-langserver");

        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        
        let (tx, rx) = channel();
        
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut size = None;
                let mut buffer = String::new();

                loop {
                    buffer.clear();
                    if reader.read_line(&mut buffer).unwrap() == 0 {
                        return; // EOF
                    }
                    let line = buffer.trim();
                    if line.is_empty() {
                        break;
                    }
                    if line.to_lowercase().starts_with("content-length: ") {
                        if let Ok(s) = line["content-length: ".len()..].parse::<usize>() {
                            size = Some(s);
                        }
                    }
                }

                if let Some(size) = size {
                    let mut body = vec![0; size];
                    reader.read_exact(&mut body).unwrap();
                    let msg: Value = serde_json::from_slice(&body).unwrap();
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
            }
        });

        Self {
            process,
            stdin: Some(stdin),
            receiver: rx,
            next_id: 1,
        }
    }

    fn send_request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;

        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.send_message(&req);
        id
    }

    fn send_notification(&mut self, method: &str, params: Value) {
        let notif = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.send_message(&notif);
    }

    fn send_message(&mut self, msg: &Value) {
        let json = serde_json::to_string(msg).unwrap();
        let content = format!("Content-Length: {}\r\n\r\n{}", json.len(), json);
        if let Some(stdin) = &mut self.stdin {
            stdin.write_all(content.as_bytes()).unwrap();
            stdin.flush().unwrap();
        }
    }

    fn wait_for_message(&self, timeout: Duration) -> Result<Value, RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }

    fn wait_for_notification(&self, method: &str, timeout: Duration) -> Value {
        let start = std::time::Instant::now();
        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                panic!("Timeout waiting for notification: {}", method);
            }
            let remaining = timeout - elapsed;
            match self.wait_for_message(remaining) {
                Ok(msg) => {
                    if let Some(m) = msg.get("method").and_then(|s| s.as_str()) {
                        if m == method {
                            return msg;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("Timeout waiting for notification: {}", method),
                Err(RecvTimeoutError::Disconnected) => panic!("Server disconnected"),
            }
        }
    }
    
    fn wait_for_response(&self, id: i64, timeout: Duration) -> Value {
        let start = std::time::Instant::now();
        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                panic!("Timeout waiting for response: {}", id);
            }
            let remaining = timeout - elapsed;
            match self.wait_for_message(remaining) {
                Ok(msg) => {
                    if let Some(i) = msg.get("id").and_then(|v| v.as_i64()) {
                        if i == id {
                            return msg;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("Timeout waiting for response: {}", id),
                Err(RecvTimeoutError::Disconnected) => panic!("Server disconnected"),
            }
        }
    }
}

impl Drop for TestLspServer {
    fn drop(&mut self) {
        // Close stdin to allow graceful shutdown
        self.stdin.take(); 
        
        // Wait for process to exit gracefully
        let start = std::time::Instant::now();
        loop {
            match self.process.try_wait() {
                Ok(Some(_)) => return, // Exited
                Ok(None) => {
                    if start.elapsed() > Duration::from_secs(2) {
                        // Timeout, kill it
                        let _ = self.process.kill();
                        let _ = self.process.wait();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => {
                    let _ = self.process.kill();
                    let _ = self.process.wait();
                    return;
                }
            }
        }
    }
}

#[test]
fn test_lsp_initialization() {
    let mut server = TestLspServer::new();
    
    let init_params = json!({
        "processId": std::process::id(),
        "rootUri": null,
        "capabilities": {}
    });
    
    let id = server.send_request("initialize", init_params);
    let response = server.wait_for_response(id, Duration::from_secs(5));
    
    assert!(response["result"]["capabilities"].is_object());
    
    server.send_notification("initialized", json!({}));
    
    let shutdown_id = server.send_request("shutdown", json!({}));
    server.wait_for_response(shutdown_id, Duration::from_secs(5));
    
    server.send_notification("exit", json!({}));
}

#[test]
fn test_lsp_diagnostics_no_error() {
    let mut server = TestLspServer::new();
    
    // Initialize
    let init_params = json!({
        "processId": std::process::id(),
        "rootUri": null,
        "capabilities": {}
    });
    let id = server.send_request("initialize", init_params);
    server.wait_for_response(id, Duration::from_secs(5));
    server.send_notification("initialized", json!({}));
    
    // Open valid file
    let uri = "file:///tmp/test_valid.px";
    let text = r#"
def hello():
    return "Hello"
"#;
    
    server.send_notification("textDocument/didOpen", json!({
        "textDocument": {
            "uri": uri,
            "languageId": "python",
            "version": 1,
            "text": text
        }
    }));
    
    // Check for diagnostics. We expect either no diagnostics or empty diagnostics.
    // We can't easily wait for "no diagnostics" without a timeout.
    // So we'll wait for a bit and see if we get any diagnostics.
    // If we do, they must be empty.
    
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(1);
    
    loop {
        if start.elapsed() >= timeout {
            break;
        }
        if let Ok(msg) = server.wait_for_message(Duration::from_millis(100)) {
            if let Some(method) = msg.get("method").and_then(|s| s.as_str()) {
                if method == "textDocument/publishDiagnostics" {
                    let diagnostics = msg["params"]["diagnostics"].as_array().unwrap();
                    assert!(diagnostics.is_empty(), "Expected no diagnostics, got: {:?}", diagnostics);
                }
            }
        }
    }
}

#[test]
fn test_lsp_diagnostics_with_error() {
    let mut server = TestLspServer::new();
    
    // Initialize
    let init_params = json!({
        "processId": std::process::id(),
        "rootUri": null,
        "capabilities": {}
    });
    let id = server.send_request("initialize", init_params);
    server.wait_for_response(id, Duration::from_secs(5));
    server.send_notification("initialized", json!({}));
    
    // Open file with type error
    let uri = "file:///tmp/test_error.px";
    let text = r#"
def Header(title: str):
    return "<h1>" + title + "</h1>"

Header(123)
"#;
    
    server.send_notification("textDocument/didOpen", json!({
        "textDocument": {
            "uri": uri,
            "languageId": "python",
            "version": 1,
            "text": text
        }
    }));
    
    // Wait for diagnostics
    // basedpyright might take a moment
    let msg = server.wait_for_notification("textDocument/publishDiagnostics", Duration::from_secs(10));
    let diagnostics = msg["params"]["diagnostics"].as_array().unwrap();
    
    assert!(!diagnostics.is_empty(), "Expected diagnostics, got none");
    
    let diag = &diagnostics[0];
    let range = &diag["range"];
    // The error is at `Header(123)`, specifically `123`.
    // Line 5 (0-indexed: 4).
    assert_eq!(range["start"]["line"], 4);
    
    // Check message contains something about type mismatch
    let message = diag["message"].as_str().unwrap();
    // basedpyright error message for this case usually mentions "Argument of type 'Literal[123]' cannot be assigned to parameter 'name' of type 'str'"
    assert!(message.contains("int") || message.contains("Literal[123]"), "Unexpected message: {}", message);
}

// ---------------------------------------------------------------------------
// Compile-error position tests: verify that the LSP server reports compile
// errors with correct line/character, matching what the compiler produces.
//
// These tests deliberately send broken JSX and assert the `range` field of
// the emitted diagnostic lands on the expected portion of the source, under
// several prefix wrappings so that line-offset math is exercised.
// ---------------------------------------------------------------------------

/// Initialize the LSP server (without testing the response contents).
fn init_server(server: &mut TestLspServer) {
    let init_params = json!({
        "processId": std::process::id(),
        "rootUri": null,
        "capabilities": {}
    });
    let id = server.send_request("initialize", init_params);
    server.wait_for_response(id, Duration::from_secs(5));
    server.send_notification("initialized", json!({}));
}

/// Open `text` as a .px document at `uri` and wait for the first
/// `publishDiagnostics` notification whose diagnostics contain an entry with
/// `source == "pythonjsx"` and a message containing `msg_substr`. Returns
/// that matching diagnostic.
fn open_and_wait_for_compile_diag(
    server: &mut TestLspServer,
    uri: &str,
    text: &str,
    msg_substr: &str,
) -> Value {
    server.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "python",
                "version": 1,
                "text": text,
            }
        }),
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut last_pythonjsx_diags: Vec<Value> = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or_else(|| Duration::from_millis(0));
        let msg = match server.wait_for_message(remaining) {
            Ok(m) => m,
            Err(_) => panic!(
                "timed out waiting for compile diagnostic containing {:?}; saw pythonjsx diagnostics: {:#?}",
                msg_substr, last_pythonjsx_diags
            ),
        };
        let method = match msg.get("method").and_then(|m| m.as_str()) {
            Some(m) => m,
            None => continue,
        };
        if method != "textDocument/publishDiagnostics" {
            continue;
        }
        let diags = match msg["params"]["diagnostics"].as_array() {
            Some(d) => d.clone(),
            None => continue,
        };
        let pythonjsx_diags: Vec<Value> = diags
            .into_iter()
            .filter(|d| d.get("source").and_then(|s| s.as_str()) == Some("pythonjsx"))
            .collect();
        if !pythonjsx_diags.is_empty() {
            last_pythonjsx_diags = pythonjsx_diags.clone();
        }
        if let Some(d) = pythonjsx_diags.iter().find(|d| {
            d.get("message")
                .and_then(|m| m.as_str())
                .map(|m| m.contains(msg_substr))
                .unwrap_or(false)
        }) {
            return d.clone();
        }
    }
}

/// Replace « and » markers with their positions (0-based line, column).
/// Returns (clean_text, start_line, start_col, end_line, end_col).
fn strip_markers(text: &str) -> (String, u64, u64, u64, u64) {
    let s_pos = text.find('«').expect("missing «");
    let after_s = &text[s_pos + '«'.len_utf8()..];
    let e_pos_rel = after_s.find('»').expect("missing »");
    let middle = &after_s[..e_pos_rel];
    let suffix = &after_s[e_pos_rel + '»'.len_utf8()..];

    let mut clean = String::with_capacity(text.len());
    clean.push_str(&text[..s_pos]);
    clean.push_str(middle);
    clean.push_str(suffix);

    let start_byte = s_pos;
    let end_byte = s_pos + middle.len();

    let byte_to_lc = |bytes: usize| -> (u64, u64) {
        let up = &clean[..bytes];
        let line = up.matches('\n').count() as u64;
        let col = match up.rfind('\n') {
            Some(p) => (bytes - (p + 1)) as u64,
            None => bytes as u64,
        };
        (line, col)
    };
    let (sl, sc) = byte_to_lc(start_byte);
    let (el, ec) = byte_to_lc(end_byte);
    (clean, sl, sc, el, ec)
}

/// Shared helper: open `source_with_markers` (containing «/»), assert the LSP
/// publishes a compile diagnostic whose range matches the marked span and
/// whose message contains `msg_substr`. Each test uses a unique temp uri.
/// `expected_severity` is the LSP severity number (1=Error, 2=Warning).
fn assert_lsp_compile_diag(
    uri: &str,
    source_with_markers: &str,
    msg_substr: &str,
    expected_severity: u64,
) {
    let (clean, sl, sc, el, ec) = strip_markers(source_with_markers);

    let mut server = TestLspServer::new();
    init_server(&mut server);

    let diag = open_and_wait_for_compile_diag(&mut server, uri, &clean, msg_substr);

    let got_severity = diag["severity"].as_u64().unwrap_or(0);
    assert_eq!(
        got_severity, expected_severity,
        "expected LSP severity {} for {:?}, got {} (diag: {:#?})",
        expected_severity, msg_substr, got_severity, diag
    );

    let range = &diag["range"];
    let got_sl = range["start"]["line"].as_u64().unwrap();
    let got_sc = range["start"]["character"].as_u64().unwrap();
    let got_el = range["end"]["line"].as_u64().unwrap();
    let got_ec = range["end"]["character"].as_u64().unwrap();
    assert_eq!(
        (got_sl, got_sc, got_el, got_ec),
        (sl, sc, el, ec),
        "LSP diagnostic range mismatch for {:?}:\nexpected ({},{})..({},{})\n     got ({},{})..({},{})\nsource:\n{}",
        msg_substr,
        sl,
        sc,
        el,
        ec,
        got_sl,
        got_sc,
        got_el,
        got_ec,
        clean,
    );
}

// LSP severity constants. See LSP spec:
//   1 = Error, 2 = Warning, 3 = Information, 4 = Hint.
const LSP_ERROR: u64 = 1;
const LSP_WARNING: u64 = 2;

#[test]
fn test_lsp_pos_mismatched_closing_tag_first_line() {
    assert_lsp_compile_diag(
        "file:///tmp/test_pos_mismatch_1.px",
        "x = <div>foo</«span»>\n",
        "Expected tag name",
        LSP_ERROR,
    );
}

#[test]
fn test_lsp_pos_mismatched_closing_tag_offset_line() {
    assert_lsp_compile_diag(
        "file:///tmp/test_pos_mismatch_2.px",
        "# one\n# two\n# three\nx = <div>foo</«span»>\n",
        "Expected tag name",
        LSP_ERROR,
    );
}

#[test]
fn test_lsp_pos_orphan_closing_tag() {
    assert_lsp_compile_diag(
        "file:///tmp/test_pos_orphan.px",
        "x = 1\ny = 2\n«</div>»\n",
        "Unexpected closing tag",
        LSP_ERROR,
    );
}

#[test]
fn test_lsp_pos_spread_missing_star_star() {
    assert_lsp_compile_diag(
        "file:///tmp/test_pos_spread.px",
        "# pad\n# pad\nx = <div «{kwargs}»></div>\n",
        "Spread attribute requires",
        LSP_ERROR,
    );
}

#[test]
fn test_lsp_pos_duplicate_attribute() {
    assert_lsp_compile_diag(
        "file:///tmp/test_pos_dup.px",
        "\nx = <div a=\"1\" «a»=\"2\"></div>\n",
        "Duplicate attribute",
        LSP_ERROR,
    );
}

#[test]
fn test_lsp_pos_unclosed_element() {
    assert_lsp_compile_diag(
        "file:///tmp/test_pos_unclosed.px",
        "# line one\n\n# line three\nx = «<div>»foo\n",
        "Unclosed element",
        LSP_ERROR,
    );
}

#[test]
fn test_lsp_pos_mismatched_at_depth() {
    assert_lsp_compile_diag(
        "file:///tmp/test_pos_depth.px",
        "# header comment\nx = <div><span>foo</«p»></span></div>\n",
        "Expected tag name 'span', got 'p'",
        LSP_ERROR,
    );
}

#[test]
fn test_lsp_pos_unknown_html_entity_in_text() {
    assert_lsp_compile_diag(
        "file:///tmp/test_pos_entity_text.px",
        "# pad\nx = <div>bla «&asdf;» bla</div>\n",
        "Unknown named HTML entity &asdf;",
        LSP_WARNING,
    );
}

#[test]
fn test_lsp_pos_unknown_html_entity_in_attribute() {
    assert_lsp_compile_diag(
        "file:///tmp/test_pos_entity_attr.px",
        "\n\nx = <div title=\"foo «&nsbp;» bar\"></div>\n",
        "Unknown named HTML entity &nsbp;",
        LSP_WARNING,
    );
}

