# Documentation

| | |
|---|---|
| [Assembly language](assembly.md) | Instruction set, directives, syntax, and the rules that surprise people |
| [Compatibility](compatibility.md) | What was checked against MARIE.js, and the one place this deviates |
| [Command line](cli.md) | Every `marie` subcommand and flag |
| [Lints](lints.md) | The catalogue, with what each one catches |
| [Architecture](architecture.md) | How the crates fit together and why |
| [Embedding](embedding.md) | Using the libraries from your own code |

Editor setup is in [`../editors/`](../editors/). API documentation is in the source:
`cargo doc --workspace --open`.

## If you are new to MARIE

Start with [Assembly language](assembly.md), then read the two rules in the
[README](../README.md#two-things-worth-knowing-immediately) — hexadecimal operands
and the leading-digit literal rule account for most of the confusion people hit.
