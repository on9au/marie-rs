// Starts mrs-lsp over stdio and hands VS Code the connection.
//
// The server does everything else; this file exists only because VS Code needs an
// extension to know that .mas is a language and how to launch a server for it.

const { workspace } = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function activate(context) {
  const command = workspace
    .getConfiguration("marie")
    .get("server.path", "marie-lsp");

  client = new LanguageClient(
    "marie",
    "MARIE assembly",
    {
      run: { command, transport: TransportKind.stdio },
      debug: { command, transport: TransportKind.stdio },
    },
    {
      documentSelector: [{ scheme: "file", language: "marie" }],
    },
  );

  context.subscriptions.push(client);
  client.start();
}

function deactivate() {
  return client && client.stop();
}

module.exports = { activate, deactivate };
