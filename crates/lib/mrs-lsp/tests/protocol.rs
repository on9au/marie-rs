//! The server driven through real protocol messages.
//!
//! The feature tests call the analysis functions directly; these go through
//! `Server::request` and `Server::notification`, so a routing mistake, a serialisation
//! mistake, or a wrong method name shows up here rather than in an editor.

use std::str::FromStr;

use lsp_server::{Notification, Request, RequestId};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use lsp_types::{
    ClientCapabilities, GeneralClientCapabilities, InitializeParams, PositionEncodingKind, Uri,
};
use mrs_lsp::{PositionEncoding, Server};
use serde_json::{Value, json};

const SOURCE: &str = "        Load  X\n        Halt\nX,      DEC 21\n";

fn uri() -> Uri {
    Uri::from_str("file:///test.mas").unwrap()
}

fn uri_string() -> String {
    "file:///test.mas".to_owned()
}

/// Builds a server with one open document.
fn server_with(source: &str) -> Server {
    let mut server = Server::new();
    server.open(uri(), source.to_owned(), 1);
    server
}

/// Sends a request and returns its result, asserting it succeeded.
fn request(server: &mut Server, method: &str, params: Value) -> Value {
    let response = server.request(Request {
        id: RequestId::from(1),
        method: method.to_owned(),
        params,
    });
    response
        .response_result
        .unwrap_or_else(|error| panic!("{method} failed: {}", error.message))
}

/// The position-params shape most requests use.
fn at(line: u32, character: u32) -> Value {
    json!({
        "textDocument": { "uri": uri_string() },
        "position": { "line": line, "character": character },
    })
}

#[test]
fn opening_a_document_publishes_diagnostics() {
    let mut server = Server::new();
    let published = server.notification(Notification::new(
        lsp_types::notification::DidOpenTextDocument::METHOD.to_owned(),
        json!({
            "textDocument": {
                "uri": uri_string(),
                "languageId": "marie",
                "version": 1,
                "text": "        Load  Missing\n",
            }
        }),
    ));
    assert_eq!(published.len(), 1);
    assert_eq!(
        published[0].method,
        lsp_types::notification::PublishDiagnostics::METHOD
    );
    let diagnostics = published[0].params["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"] == "asm::unknown-label"),
        "{diagnostics:#?}"
    );
    assert_eq!(published[0].params["version"], 1);
}

#[test]
fn changing_a_document_republishes_and_can_clear_the_squiggles() {
    let mut server = server_with("        Load  Missing\n");
    let published = server.notification(Notification::new(
        lsp_types::notification::DidChangeTextDocument::METHOD.to_owned(),
        json!({
            "textDocument": { "uri": uri_string(), "version": 2 },
            "contentChanges": [{ "text": SOURCE }],
        }),
    ));
    let diagnostics = published[0].params["diagnostics"].as_array().unwrap();
    assert!(diagnostics.is_empty(), "fixed file: {diagnostics:#?}");
    assert_eq!(published[0].params["version"], 2);
    // And the new text is what later requests see.
    assert_eq!(server.document(&uri()).unwrap().text, SOURCE);
}

#[test]
fn closing_a_document_clears_its_diagnostics_and_forgets_it() {
    let mut server = server_with("        Load  Missing\n");
    let published = server.notification(Notification::new(
        lsp_types::notification::DidCloseTextDocument::METHOD.to_owned(),
        json!({ "textDocument": { "uri": uri_string() } }),
    ));
    assert_eq!(published.len(), 1);
    assert!(
        published[0].params["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a closed file must not keep squiggles"
    );
    assert!(server.document(&uri()).is_none());
}

#[test]
fn a_change_to_an_unknown_document_opens_it_rather_than_being_dropped() {
    // Editors can race didChange ahead of didOpen after a restart.
    let mut server = Server::new();
    let published = server.notification(Notification::new(
        lsp_types::notification::DidChangeTextDocument::METHOD.to_owned(),
        json!({
            "textDocument": { "uri": uri_string(), "version": 7 },
            "contentChanges": [{ "text": SOURCE }],
        }),
    ));
    assert_eq!(published.len(), 1);
    assert!(server.document(&uri()).is_some());
}

#[test]
fn every_request_is_routed_and_answers_with_the_right_shape() {
    let mut server = server_with(SOURCE);

    let hover = request(
        &mut server,
        lsp_types::request::HoverRequest::METHOD,
        at(0, 8),
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Load")
    );

    let definition = request(
        &mut server,
        lsp_types::request::GotoDefinition::METHOD,
        at(0, 14),
    );
    assert_eq!(definition["range"]["start"]["line"], 2);

    let references = request(
        &mut server,
        lsp_types::request::References::METHOD,
        json!({
            "textDocument": { "uri": uri_string() },
            "position": { "line": 0, "character": 14 },
            "context": { "includeDeclaration": true },
        }),
    );
    assert_eq!(references.as_array().unwrap().len(), 2);

    let highlight = request(
        &mut server,
        lsp_types::request::DocumentHighlightRequest::METHOD,
        at(0, 14),
    );
    assert_eq!(highlight.as_array().unwrap().len(), 2);

    let symbols = request(
        &mut server,
        lsp_types::request::DocumentSymbolRequest::METHOD,
        json!({ "textDocument": { "uri": uri_string() } }),
    );
    assert_eq!(symbols[0]["name"], "X");

    let completion = request(
        &mut server,
        lsp_types::request::Completion::METHOD,
        json!({
            "textDocument": { "uri": uri_string() },
            "position": { "line": 1, "character": 8 },
        }),
    );
    assert!(!completion.as_array().unwrap().is_empty());

    let tokens = request(
        &mut server,
        lsp_types::request::SemanticTokensFullRequest::METHOD,
        json!({ "textDocument": { "uri": uri_string() } }),
    );
    // Five integers per token.
    assert_eq!(tokens["data"].as_array().unwrap().len() % 5, 0);

    let hints = request(
        &mut server,
        lsp_types::request::InlayHintRequest::METHOD,
        json!({
            "textDocument": { "uri": uri_string() },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 2, "character": 0 },
            },
        }),
    );
    assert_eq!(hints.as_array().unwrap().len(), 3);
    assert_eq!(hints[0]["label"], "000: 1002");

    let actions = request(
        &mut server,
        lsp_types::request::CodeActionRequest::METHOD,
        json!({
            "textDocument": { "uri": uri_string() },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 2, "character": 0 },
            },
            "context": { "diagnostics": [] },
        }),
    );
    assert!(actions.as_array().unwrap().is_empty(), "clean file");
}

#[test]
fn prepare_rename_and_rename_go_through_the_protocol() {
    let mut server = server_with(SOURCE);

    let prepared = request(
        &mut server,
        lsp_types::request::PrepareRenameRequest::METHOD,
        at(0, 14),
    );
    assert_eq!(prepared["placeholder"], "X");

    let edit = request(
        &mut server,
        lsp_types::request::Rename::METHOD,
        json!({
            "textDocument": { "uri": uri_string() },
            "position": { "line": 0, "character": 14 },
            "newName": "Value",
        }),
    );
    let edits = edit["changes"][uri_string()].as_array().unwrap();
    assert_eq!(edits.len(), 2);
}

#[test]
fn an_invalid_rename_is_an_error_response_rather_than_a_panic() {
    let mut server = server_with(SOURCE);
    let response = server.request(Request {
        id: RequestId::from(1),
        method: lsp_types::request::Rename::METHOD.to_owned(),
        params: json!({
            "textDocument": { "uri": uri_string() },
            "position": { "line": 0, "character": 14 },
            "newName": "1Bad",
        }),
    });
    let error = response.response_result.unwrap_err();
    assert!(error.message.contains("digit"), "{}", error.message);
}

#[test]
fn a_request_for_an_unopened_document_answers_null() {
    let mut server = Server::new();
    let hover = request(
        &mut server,
        lsp_types::request::HoverRequest::METHOD,
        at(0, 0),
    );
    assert_eq!(hover, Value::Null);
}

#[test]
fn malformed_parameters_produce_an_error_rather_than_a_panic() {
    let mut server = server_with(SOURCE);
    let response = server.request(Request {
        id: RequestId::from(1),
        method: lsp_types::request::HoverRequest::METHOD.to_owned(),
        params: json!({ "nonsense": true }),
    });
    assert!(response.response_result.is_err());
}

#[test]
fn an_unknown_method_answers_null_instead_of_failing() {
    let mut server = server_with(SOURCE);
    let result = request(&mut server, "textDocument/somethingElse", json!({}));
    assert_eq!(result, Value::Null);
}

#[test]
fn an_unknown_notification_is_ignored() {
    let mut server = server_with(SOURCE);
    let published = server.notification(Notification::new("$/setTrace".to_owned(), json!({})));
    assert!(published.is_empty());
}

#[test]
fn the_encoding_is_negotiated_from_the_client_capabilities() {
    let with_utf8 = InitializeParams {
        capabilities: ClientCapabilities {
            general: Some(GeneralClientCapabilities {
                position_encodings: Some(vec![
                    PositionEncodingKind::UTF16,
                    PositionEncodingKind::UTF8,
                ]),
                ..GeneralClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        },
        ..InitializeParams::default()
    };
    assert_eq!(Server::negotiate(&with_utf8), PositionEncoding::Utf8);

    // A client that says nothing gets the protocol default.
    assert_eq!(
        Server::negotiate(&InitializeParams::default()),
        PositionEncoding::Utf16
    );
}

#[test]
fn the_advertised_capabilities_match_what_is_implemented() {
    let capabilities = Server::capabilities(PositionEncoding::Utf16);
    assert_eq!(
        capabilities.position_encoding,
        Some(PositionEncodingKind::UTF16)
    );
    assert!(capabilities.hover_provider.is_some());
    assert!(capabilities.definition_provider.is_some());
    assert!(capabilities.references_provider.is_some());
    assert!(capabilities.document_highlight_provider.is_some());
    assert!(capabilities.document_symbol_provider.is_some());
    assert!(capabilities.rename_provider.is_some());
    assert!(capabilities.completion_provider.is_some());
    assert!(capabilities.semantic_tokens_provider.is_some());
    assert!(capabilities.inlay_hint_provider.is_some());
    assert!(capabilities.code_action_provider.is_some());
    // Full sync is what the loop implements; advertising incremental would desync.
    assert!(matches!(
        capabilities.text_document_sync,
        Some(lsp_types::TextDocumentSyncCapability::Kind(
            lsp_types::TextDocumentSyncKind::FULL
        ))
    ));
}

#[test]
fn a_document_with_multi_byte_text_reports_utf16_columns() {
    // The whole point of the encoding module: a non-ASCII label must not shift ranges.
    let source = "caf\u{e9},  Load  caf\u{e9}\n";
    let mut server = Server::new();
    server.set_encoding(PositionEncoding::Utf16);
    server.open(uri(), source.to_owned(), 1);

    let definition = request(
        &mut server,
        lsp_types::request::GotoDefinition::METHOD,
        at(0, 14),
    );
    // `café` is four UTF-16 units, so the definition ends at column 4, not 5.
    assert_eq!(definition["range"]["start"]["character"], 0);
    assert_eq!(definition["range"]["end"]["character"], 4);
}
