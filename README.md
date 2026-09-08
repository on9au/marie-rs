# marie-rs

A [MARIE][marie] assembler, virtual machine, linter and language server in Rust.

MARIE is the teaching architecture from Null & Lobur's *Essentials of Computer
Organization and Architecture*: sixteen-bit words, a 4096-word address space, one
accumulator, fifteen instructions. This implements the dialect used by
[MARIE.js][mariejs], which is the version most courses actually run, and is
deliberately bug-compatible with it — see [Compatibility](docs/compatibility.md).

```console
$ marie run   examples/sum.mas     # a working program
$ marie lint  examples/hazards.mas # assembles cleanly, still wrong
$ marie debug examples/sum.mas     # step through it, forwards and backwards
```

## What is here

| Crate | |
|---|---|
| [`mrs-core`](crates/lib/mrs-core) | The architecture: words, addresses, instruction encoding, directives, literals, the binary image format, the display |
| [`mrs-vm`](crates/lib/mrs-vm) | The machine: microcoded execution, reverse execution, breakpoints, I/O devices |
| [`mrs-asm`](crates/lib/mrs-asm) | The assembler, with spans, an extensible diagnostic system and a symbol table |
| [`mrs-lint`](crates/lib/mrs-lint) | Lints that need a control-flow graph |
| [`mrs-lsp`](crates/lib/mrs-lsp) | Language-server features over the above |
| [`marie`](crates/bin/marie) | The command-line tool |
| [`marie-lsp`](crates/bin/marie-lsp) | The language server binary |

## Install

```console
$ git clone https://github.com/on9au/marie-rs && cd marie-rs
$ cargo install --path crates/bin/marie
$ cargo install --path crates/bin/marie-lsp
```

Or `cargo build --release` and use `target/release/marie`.

## The command line

```console
$ marie asm    program.mas --listing      # assemble, show address/word/source
$ marie asm    program.mas -o out.bin     # write a MARIE.js-compatible image
$ marie run    program.mas                # execute it
$ marie run    program.bin --display      # run an image, drawing the 16x16 display
$ marie lint   program.mas --deny falls-into-data
$ marie debug  program.mas                # step forwards *and backwards*
$ marie disasm program.bin                # read an image back as assembly
```

Full reference: [docs/cli.md](docs/cli.md).

## Two things worth knowing immediately

Both bite everyone who writes MARIE, and both are inherited from MARIE.js rather
than invented here.

**Instruction operands are hexadecimal.** `Add 123` adds the word at address
`0x123`, not decimal 123. Only `DEC`, `OCT` and `HEX` literals use another base.

**An operand is a literal only if it starts with a decimal digit.** The rule is
`/^\d[0-9a-fA-F]*$/`, so `Load 1A` reads address `0x1A` while `Load A1` is a
reference to a label named `A1`. This is why the non-zero skip condition must be
written `Skipcond 0C00` — bare `C00` is read as a label and fails to assemble.

The linter and the language server both catch these, and the language server
offers the leading zero as a quick fix.

## Editors

`marie-lsp` gives diagnostics, hover, go-to-definition, references, rename,
document symbols, completion, semantic tokens, quick fixes, and inlay hints that
show each line's address and assembled word. Configuration for Neovim and VS Code
is in [`editors/`](editors/), and `scripts/lsp-smoke.py` exercises the server
without an editor.

## Documentation

| | |
|---|---|
| [Assembly language](docs/assembly.md) | Instruction set, directives, syntax, and the rules that surprise people |
| [Compatibility](docs/compatibility.md) | What was checked against MARIE.js, and the one place this deviates |
| [Command line](docs/cli.md) | Every subcommand and flag |
| [Lints](docs/lints.md) | The catalogue, with what each one catches |
| [Architecture](docs/architecture.md) | How the crates fit together and why |
| [Embedding](docs/embedding.md) | Using the libraries: custom lints, diagnostics, display frontends |

API documentation: `cargo doc --workspace --open`.

## Testing

```console
$ cargo test --workspace
```

The suite includes MARIE.js's own assembler and simulator tests, ported directly,
so a divergence shows up as a failing test rather than as a program that quietly
computes something else.

## Licence

MIT or Apache-2.0, at your option.

[marie]: https://en.wikipedia.org/wiki/MARIE_(computer_architecture)
[mariejs]: https://marie.js.org
