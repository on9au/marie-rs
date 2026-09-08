//! The `marie-lsp` language server binary.
//!
//! Speaks LSP over stdio, which is how every editor launches a language server.
//!
//! ```console
//! $ marie-lsp
//! ```

use std::error::Error;

use lsp_server::Connection;
use mrs_lsp::Server;

fn main() -> Result<(), Box<dyn Error>> {
    // Anything written to stdout is protocol traffic, so logging goes to stderr.
    eprintln!("marie-lsp starting");

    let (connection, io_threads) = Connection::stdio();

    // The capabilities depend on the encoding the client offers, so the initialisation
    // parameters have to be read before they can be answered.
    let (id, params) = connection.initialize_start()?;
    let params: lsp_types::InitializeParams = serde_json::from_value(params)?;
    let encoding = Server::negotiate(&params);

    let result = serde_json::json!({
        "capabilities": Server::capabilities(encoding),
        "serverInfo": { "name": "marie-lsp", "version": env!("CARGO_PKG_VERSION") },
    });
    connection.initialize_finish(id, result)?;

    eprintln!("marie-lsp ready ({encoding:?})");
    // `run` consumes the connection, so its channels are closed by the time the I/O
    // threads are joined below.
    Server::run(connection, params)?;

    io_threads.join()?;
    eprintln!("marie-lsp stopped");
    Ok(())
}
