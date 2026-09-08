//! The language server binary, spoken to over stdio the way an editor does.
//!
//! The in-process tests in `mrs-lsp` cover routing and the features. What only a real
//! process can cover is the framing, the initialise handshake, and the shutdown
//! sequence — the last of which is where a deadlock hid: the loop borrowed the
//! connection rather than owning it, so its sender stayed alive and joining the I/O
//! threads blocked forever. Nothing short of running the binary catches that.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

/// A running language server.
struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Server {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_marie-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the server should start");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    /// Sends one message with LSP's `Content-Length` framing.
    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).expect("serialise");
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("header");
        self.stdin.write_all(&body).expect("body");
        self.stdin.flush().expect("flush");
    }

    /// Reads one message, parsing the framing headers.
    fn receive(&mut self) -> Value {
        let mut length = None;
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("header line");
            assert_ne!(read, 0, "the server closed the connection early");
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                length = Some(rest.trim().parse::<usize>().expect("a length"));
            }
        }
        let length = length.expect("a Content-Length header");
        let mut body = vec![0u8; length];
        self.stdout.read_exact(&mut body).expect("body");
        serde_json::from_slice(&body).expect("valid JSON")
    }

    /// Performs the initialise handshake, returning the server's capabilities.
    fn initialize(&mut self, encodings: &[&str]) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": null,
                "capabilities": { "general": { "positionEncodings": encodings } },
            }
        }));
        let response = self.receive();
        self.send(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
        response["result"].clone()
    }

    /// Shuts the server down and asserts it exited cleanly.
    fn shutdown(mut self) {
        self.send(json!({ "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null }));
        let response = self.receive();
        assert_eq!(response["id"], 99);
        self.send(json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
        // A real client closes the pipe; the reader thread ends on EOF.
        drop(self.stdin);

        // Poll rather than block, so a regression fails the test instead of hanging it.
        for _ in 0..100 {
            if let Some(status) = self.child.try_wait().expect("wait") {
                assert!(status.success(), "the server exited with {status}");
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        panic!("the server did not exit within five seconds");
    }
}

const SOURCE: &str = "        Load  First\n        Skipcond C00\n        Halt\nFirst,  DEC 21\n";

/// Opens a document and returns the diagnostics the server publishes for it.
fn open_and_collect(server: &mut Server, text: &str) -> Vec<Value> {
    server.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": "file:///demo.mas",
            "languageId": "marie",
            "version": 1,
            "text": text,
        }}
    }));
    let notification = server.receive();
    assert_eq!(notification["method"], "textDocument/publishDiagnostics");
    notification["params"]["diagnostics"]
        .as_array()
        .expect("an array")
        .clone()
}

#[test]
fn the_server_initialises_and_advertises_its_capabilities() {
    let mut server = Server::start();
    let result = server.initialize(&["utf-16"]);
    assert_eq!(result["serverInfo"]["name"], "marie-lsp");
    let capabilities = &result["capabilities"];
    assert!(capabilities["hoverProvider"].as_bool().unwrap_or(false));
    assert!(!capabilities["definitionProvider"].is_null());
    assert!(!capabilities["semanticTokensProvider"].is_null());
    assert!(!capabilities["inlayHintProvider"].is_null());
    server.shutdown();
}

#[test]
fn the_server_shuts_down_cleanly() {
    // The regression test for the join deadlock: `shutdown` fails rather than hangs.
    let mut server = Server::start();
    server.initialize(&["utf-16"]);
    server.shutdown();
}

#[test]
fn utf8_is_negotiated_when_the_client_offers_it() {
    let mut server = Server::start();
    let result = server.initialize(&["utf-16", "utf-8"]);
    assert_eq!(result["capabilities"]["positionEncoding"], "utf-8");
    server.shutdown();
}

#[test]
fn utf16_is_used_when_the_client_offers_nothing_else() {
    let mut server = Server::start();
    let result = server.initialize(&["utf-16"]);
    assert_eq!(result["capabilities"]["positionEncoding"], "utf-16");
    server.shutdown();
}

#[test]
fn opening_a_document_publishes_diagnostics() {
    let mut server = Server::start();
    server.initialize(&["utf-16"]);
    let diagnostics = open_and_collect(&mut server, SOURCE);
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"] == "asm::unknown-label"),
        "{diagnostics:#?}"
    );
    server.shutdown();
}

#[test]
fn a_quick_fix_is_offered_for_the_skipcond_trap() {
    let mut server = Server::start();
    server.initialize(&["utf-16"]);
    open_and_collect(&mut server, SOURCE);

    server.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": "file:///demo.mas" },
            "range": {
                "start": { "line": 1, "character": 17 },
                "end": { "line": 1, "character": 20 },
            },
            "context": { "diagnostics": [] },
        }
    }));
    let response = server.receive();
    let actions = response["result"].as_array().expect("actions");
    assert_eq!(actions.len(), 1);
    assert!(
        actions[0]["title"].as_str().unwrap().contains("0C00"),
        "{actions:#?}"
    );
    server.shutdown();
}

#[test]
fn editing_a_document_republishes_diagnostics() {
    let mut server = Server::start();
    server.initialize(&["utf-16"]);
    let before = open_and_collect(&mut server, SOURCE);
    assert!(!before.is_empty());

    server.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": "file:///demo.mas", "version": 2 },
            "contentChanges": [{
                "text": "        Load  First\n        Halt\nFirst,  DEC 21\n"
            }],
        }
    }));
    let notification = server.receive();
    let after = notification["params"]["diagnostics"].as_array().unwrap();
    assert!(after.is_empty(), "the fixed file is clean: {after:#?}");
    server.shutdown();
}

#[test]
fn hover_answers_over_the_wire() {
    let mut server = Server::start();
    server.initialize(&["utf-16"]);
    open_and_collect(&mut server, SOURCE);

    server.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": "file:///demo.mas" },
            "position": { "line": 0, "character": 8 },
        }
    }));
    let response = server.receive();
    let contents = response["result"]["contents"]["value"]
        .as_str()
        .expect("markup");
    assert!(contents.contains("Load"), "{contents}");
    server.shutdown();
}

#[test]
fn inlay_hints_carry_the_address_and_word() {
    let mut server = Server::start();
    server.initialize(&["utf-16"]);
    open_and_collect(
        &mut server,
        "        Load  X\n        Halt\nX,      DEC 21\n",
    );

    server.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "textDocument/inlayHint",
        "params": {
            "textDocument": { "uri": "file:///demo.mas" },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 2, "character": 0 },
            },
        }
    }));
    let response = server.receive();
    let hints = response["result"].as_array().expect("hints");
    assert_eq!(hints[0]["label"], "000: 1002");
    server.shutdown();
}
