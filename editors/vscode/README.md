# VS Code

The server is the whole implementation; this extension only tells VS Code that
`.mas` is a language and how to launch `marie-lsp` for it.

```console
$ cd editors/vscode
$ npm install
$ ln -s "$PWD" ~/.vscode/extensions/marie-lsp
```

Restart VS Code and open a `.mas` file. If `marie-lsp` is not on your `PATH`, set
`marie.server.path` in settings to the built binary, for example
`~/Projects/marie-rs/target/release/marie-lsp`.
