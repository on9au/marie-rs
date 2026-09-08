# Editor setup

`marie-lsp` speaks LSP over stdio, so any client can drive it. The only thing an
editor needs to be told is that `.mas` is a language and how to start the server —
which is all the configuration in here does.

| Editor | See |
|---|---|
| Neovim 0.11+ | [`nvim/`](nvim/) |
| VS Code | [`vscode/`](vscode/) |

## Checking the server without an editor

Useful when something is not attaching and you want to know which side is at fault:

```console
$ cargo run --release -p marie-lsp
```

It waits for LSP framing on stdin, so nothing happening is correct. To drive it by
hand, `scripts/lsp-smoke.py` runs an initialise handshake, opens a file and prints
the diagnostics, hover and quick fixes it gets back:

```console
$ python3 scripts/lsp-smoke.py examples/demo.mas
```

## What to try

`examples/demo.mas` has two deliberate mistakes and an unused label:

- hover a mnemonic, a label, or a hexadecimal operand
- go to definition on `First`, then find references
- rename `Total`
- quick-fix `Skipcond C00` — it is read as a *label*, and the fix adds the leading
  zero that makes it a literal again
- inlay hints show each line's address and assembled word, and appear once the
  file assembles cleanly
