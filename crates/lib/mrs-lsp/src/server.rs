//! The protocol loop.
//!
//! Synchronous and single-threaded: a MARIE program is at most 4096 words, so analysis
//! is far too fast to need concurrency, and a straight-line loop is much easier to be
//! sure of than an async one.
//!
//! Text is synchronised in full rather than incrementally. Incremental sync exists to
//! avoid resending large files; these files are small, and applying ranged edits by
//! hand is a well-known source of desynchronisation bugs.

use std::collections::HashMap;
use std::error::Error;

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentHighlightRequest, DocumentSymbolRequest, GotoDefinition,
    HoverRequest, InlayHintRequest, PrepareRenameRequest, References, Rename, Request as _,
    SemanticTokensFullRequest,
};
use lsp_types::{
    CodeActionProviderCapability, CompletionOptions, DocumentSymbolResponse,
    GotoDefinitionResponse, InitializeParams, OneOf, PublishDiagnosticsParams,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensServerCapabilities,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
    WorkDoneProgressOptions,
};
use mrs_lint::Linter;

use crate::document::Document;
use crate::encoding::PositionEncoding;
use crate::features;

/// The server state.
pub struct Server {
    documents: HashMap<Uri, Document>,
    linter: Linter<'static>,
    encoding: PositionEncoding,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    /// Creates a server with the standard lint set.
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            linter: Linter::new(),
            encoding: PositionEncoding::default(),
        }
    }

    /// Returns the capabilities to advertise, given what the client supports.
    pub fn capabilities(encoding: PositionEncoding) -> ServerCapabilities {
        ServerCapabilities {
            position_encoding: Some(encoding.kind()),
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            hover_provider: Some(true.into()),
            definition_provider: Some(OneOf::Left(true)),
            references_provider: Some(OneOf::Left(true)),
            document_highlight_provider: Some(OneOf::Left(true)),
            document_symbol_provider: Some(OneOf::Left(true)),
            rename_provider: Some(OneOf::Right(lsp_types::RenameOptions {
                prepare_provider: Some(true),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            })),
            completion_provider: Some(CompletionOptions {
                // A label becomes suggestible as soon as a space is typed.
                trigger_characters: Some(vec![" ".to_owned(), ",".to_owned()]),
                ..CompletionOptions::default()
            }),
            semantic_tokens_provider: Some(
                SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
                    legend: features::tokens::legend(),
                    full: Some(SemanticTokensFullOptions::Bool(true)),
                    range: None,
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
            ),
            inlay_hint_provider: Some(OneOf::Left(true)),
            code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
            ..ServerCapabilities::default()
        }
    }

    /// Reads the encoding the client is willing to speak.
    pub fn negotiate(params: &InitializeParams) -> PositionEncoding {
        PositionEncoding::negotiate(
            params
                .capabilities
                .general
                .as_ref()
                .and_then(|general| general.position_encodings.as_deref()),
        )
    }

    /// Runs the loop until the client asks to shut down.
    ///
    /// Takes the connection **by value** on purpose: its sender must be dropped before
    /// `IoThreads::join`, or the writer thread never sees its channel close and the
    /// join blocks forever.
    pub fn run(connection: Connection, params: InitializeParams) -> Result<(), Box<dyn Error>> {
        let mut server = Self::new();
        server.encoding = Self::negotiate(&params);

        for message in &connection.receiver {
            match message {
                Message::Request(request) => {
                    if connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    let response = server.request(request);
                    connection.sender.send(Message::Response(response))?;
                }
                Message::Notification(notification) => {
                    for published in server.notification(notification) {
                        connection.sender.send(Message::Notification(published))?;
                    }
                }
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    /// Handles a notification, returning any diagnostics to publish.
    pub fn notification(&mut self, notification: Notification) -> Vec<Notification> {
        let method = notification.method.clone();
        if method == DidOpenTextDocument::METHOD {
            let Ok(params) = cast_notification::<DidOpenTextDocument>(notification) else {
                return Vec::new();
            };
            let document = Document::new(
                params.text_document.text,
                params.text_document.version,
                &self.linter,
            );
            self.documents
                .insert(params.text_document.uri.clone(), document);
            return self.publish(&params.text_document.uri);
        }
        if method == DidChangeTextDocument::METHOD {
            let Ok(mut params) = cast_notification::<DidChangeTextDocument>(notification) else {
                return Vec::new();
            };
            // Full sync, so the last change carries the whole document.
            let Some(change) = params.content_changes.pop() else {
                return Vec::new();
            };
            let uri = params.text_document.uri.clone();
            let version = params.text_document.version;
            match self.documents.get_mut(&uri) {
                Some(document) => document.update(change.text, version, &self.linter),
                None => {
                    self.documents.insert(
                        uri.clone(),
                        Document::new(change.text, version, &self.linter),
                    );
                }
            }
            return self.publish(&uri);
        }
        if method == DidCloseTextDocument::METHOD
            && let Ok(params) = cast_notification::<DidCloseTextDocument>(notification)
        {
            self.documents.remove(&params.text_document.uri);
            // Clear the squiggles for a file nobody is looking at any more.
            return vec![diagnostics_notification(
                params.text_document.uri,
                Vec::new(),
                None,
            )];
        }
        Vec::new()
    }

    /// Builds the diagnostics notification for one document.
    fn publish(&self, uri: &Uri) -> Vec<Notification> {
        let Some(document) = self.documents.get(uri) else {
            return Vec::new();
        };
        vec![diagnostics_notification(
            uri.clone(),
            features::diagnostics::diagnostics(document, uri, self.encoding),
            Some(document.version),
        )]
    }

    /// Answers a request.
    pub fn request(&mut self, request: Request) -> Response {
        let id = request.id.clone();
        match self.dispatch(request) {
            Ok(value) => Response {
                id,
                response_result: Ok(value),
            },
            Err(message) => {
                Response::new_err(id, lsp_server::ErrorCode::RequestFailed as i32, message)
            }
        }
    }

    /// Routes a request to its feature, returning the JSON result.
    fn dispatch(&mut self, request: Request) -> Result<serde_json::Value, String> {
        let method = request.method.clone();
        let encoding = self.encoding;

        macro_rules! document {
            ($uri:expr) => {
                match self.documents.get($uri) {
                    Some(document) => document,
                    // A request for a file we were never told about is not an error;
                    // answering "nothing" is what the client expects.
                    None => return Ok(serde_json::Value::Null),
                }
            };
        }

        if method == HoverRequest::METHOD {
            let (_, params) = cast::<HoverRequest>(request)?;
            let uri = &params.text_document_position_params.text_document.uri;
            let document = document!(uri);
            let result = features::hover::hover(
                document,
                params.text_document_position_params.position,
                encoding,
            );
            return json(result);
        }
        if method == GotoDefinition::METHOD {
            let (_, params) = cast::<GotoDefinition>(request)?;
            let uri = params
                .text_document_position_params
                .text_document
                .uri
                .clone();
            let document = document!(&uri);
            let result = features::navigation::definition(
                document,
                &uri,
                params.text_document_position_params.position,
                encoding,
            )
            .map(GotoDefinitionResponse::Scalar);
            return json(result);
        }
        if method == References::METHOD {
            let (_, params) = cast::<References>(request)?;
            let uri = params.text_document_position.text_document.uri.clone();
            let document = document!(&uri);
            let result = features::navigation::references(
                document,
                &uri,
                params.text_document_position.position,
                encoding,
                params.context.include_declaration,
            );
            return json(result);
        }
        if method == DocumentHighlightRequest::METHOD {
            let (_, params) = cast::<DocumentHighlightRequest>(request)?;
            let uri = &params.text_document_position_params.text_document.uri;
            let document = document!(uri);
            let result = features::navigation::highlight(
                document,
                params.text_document_position_params.position,
                encoding,
            );
            return json(result);
        }
        if method == PrepareRenameRequest::METHOD {
            let (_, params) = cast::<PrepareRenameRequest>(request)?;
            let document = document!(&params.text_document.uri);
            let result = features::navigation::prepare_rename(document, params.position, encoding);
            return json(result);
        }
        if method == Rename::METHOD {
            let (_, params) = cast::<Rename>(request)?;
            let uri = params.text_document_position.text_document.uri.clone();
            let document = document!(&uri);
            let result = features::navigation::rename(
                document,
                &uri,
                params.text_document_position.position,
                &params.new_name,
                encoding,
            )
            .map_err(str::to_owned)?;
            return json(result);
        }
        if method == DocumentSymbolRequest::METHOD {
            let (_, params) = cast::<DocumentSymbolRequest>(request)?;
            let document = document!(&params.text_document.uri);
            let symbols = features::symbols::document_symbols(document, encoding);
            return json(Some(DocumentSymbolResponse::Nested(symbols)));
        }
        if method == Completion::METHOD {
            let (_, params) = cast::<Completion>(request)?;
            let document = document!(&params.text_document_position.text_document.uri);
            let items = features::completion::completion(
                document,
                params.text_document_position.position,
                encoding,
            );
            return json(Some(items));
        }
        if method == SemanticTokensFullRequest::METHOD {
            let (_, params) = cast::<SemanticTokensFullRequest>(request)?;
            let document = document!(&params.text_document.uri);
            let tokens = features::tokens::semantic_tokens(document, encoding);
            return json(Some(tokens));
        }
        if method == InlayHintRequest::METHOD {
            let (_, params) = cast::<InlayHintRequest>(request)?;
            let document = document!(&params.text_document.uri);
            let hints = features::hints::inlay_hints(document, params.range, encoding);
            return json(Some(hints));
        }
        if method == CodeActionRequest::METHOD {
            let (_, params) = cast::<CodeActionRequest>(request)?;
            let uri = params.text_document.uri.clone();
            let document = document!(&uri);
            let actions = features::actions::code_actions(document, &uri, params.range, encoding);
            return json(Some(actions));
        }

        Ok(serde_json::Value::Null)
    }

    /// Inserts a document directly, for tests and embedders.
    pub fn open(&mut self, uri: Uri, text: String, version: i32) {
        let document = Document::new(text, version, &self.linter);
        self.documents.insert(uri, document);
    }

    /// Returns an open document.
    pub fn document(&self, uri: &Uri) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// Sets the position encoding, for tests.
    pub fn set_encoding(&mut self, encoding: PositionEncoding) {
        self.encoding = encoding;
    }

    /// The negotiated position encoding.
    pub fn encoding(&self) -> PositionEncoding {
        self.encoding
    }
}

/// Serialises a feature's result.
fn json<T: serde::Serialize>(value: T) -> Result<serde_json::Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

/// Extracts a typed request, turning a mismatch into a message rather than a panic.
fn cast<R>(request: Request) -> Result<(RequestId, R::Params), String>
where
    R: lsp_types::request::Request,
    R::Params: serde::de::DeserializeOwned,
{
    request.extract(R::METHOD).map_err(|error| match error {
        ExtractError::MethodMismatch(request) => {
            format!("method mismatch for {}", request.method)
        }
        ExtractError::JsonError { method, error } => format!("bad params for {method}: {error}"),
    })
}

/// Extracts a typed notification.
fn cast_notification<N>(notification: Notification) -> Result<N::Params, ()>
where
    N: lsp_types::notification::Notification,
    N::Params: serde::de::DeserializeOwned,
{
    notification.extract(N::METHOD).map_err(|_| ())
}

/// Builds a `textDocument/publishDiagnostics` notification.
fn diagnostics_notification(
    uri: Uri,
    diagnostics: Vec<lsp_types::Diagnostic>,
    version: Option<i32>,
) -> Notification {
    Notification::new(
        PublishDiagnostics::METHOD.to_owned(),
        PublishDiagnosticsParams {
            uri,
            diagnostics,
            version,
        },
    )
}
