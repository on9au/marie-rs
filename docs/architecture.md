# Architecture

```text
                       marie          marie-lsp        crates/bin
                         │                │
        ┌────────────────┼────────┬───────┴──┐
        │                │        │          │
     mrs-lint  ◄──────  mrs-asm   │       mrs-lsp
        │                │        │          │
        └──────────┬─────┴────────┴──────────┘
                   │              │
                mrs-core       mrs-vm  ──►  mrs-core
```

Dependencies point one way. `mrs-asm` does **not** depend on `mrs-vm`: assembling
and executing are separate concerns that happen to share an instruction encoding,
and that separation is why the assembler can be embedded in an editor without
dragging a virtual machine along.

## The crates

### `mrs-core` — the architecture

The word type, the address space, the instruction encoding, the directives, the
literal grammar, the binary image format, and the display's geometry and pixel
format. No execution, no I/O, no allocation on the hot paths.

The rule for what belongs here is *"is this true of the machine?"*. The display's
memory map is, so it lives here even though nothing in the instruction set mentions
it. Reading a `.bin` is a pure encoding of a `MemoryImage`, so that lives here too;
the file handling does not.

`MemoryAddress` and `Value` are newtypes whose constructors either mask or reject,
so an existing `MemoryAddress` is always a valid index — the type system carries the
12-bit invariant instead of every call site re-checking it.

### `mrs-vm` — the machine

Two decisions shape it.

**Microcode as data.** Every instruction is a `&'static [MicroOp]` of register
transfers preceded by a shared fetch. This buys three things at once: the debugger
can single-step *within* an instruction, each operation has one well-defined effect
so the history journal can reverse it, and the sequences can be read side by side
with the MARIE.js microcode they mirror.

**Type states.** `MarieVM<IO, S>` where `S` is `Ready`, `Running`, `Stepping` or
`Terminated`. Outcomes own the VM and hand it back in its new state, so stepping a
terminated machine is a compile error rather than a runtime check.

Reverse execution is real. Each micro-operation records the value it overwrote, so
`step_back` restores it; the I/O trait has `unread_input`/`unwrite_output` so a
device can decline, and the debugger reports it rather than losing data silently.

### `mrs-asm` — the assembler

Split into passes so tooling can enter at any layer: a line splitter, a parser
producing a lossless syntax tree, a symbol table with definition *and* reference
sites, a two-way source map, and an extensible diagnostic system.

It never stops at the first error and never panics on malformed input, because the
state an editor sees most often is broken source. Every item yields a word even when
it fails to assemble, so the tree, the word list and the source map stay aligned.

The line splitter is hand-written and reproduces MARIE.js's regex *including its
backtracking order*, because that regex has two lazy quantifiers and the split is
decided by backtracking rather than by scanning. `Load/c` is `Load` plus a comment;
`/c` alone is an operator named `/c`. Approximating with "split on the first slash"
would quietly diverge.

### `mrs-lint` — the linter

A control-flow graph over the assembled words, plus lints built on it. The graph is
explicit about what it cannot know: `Cfg::exact()` reports whether the program has
indirect transfers, and lints whose conclusions would be unsound without that stay
quiet. See [Lints](lints.md).

### `mrs-lsp` — the language server

Mostly a translation layer, because the assembler already produces what a language
server needs. The part that is not translation is `encoding`: LSP columns count code
units in a negotiated encoding while every span here is a byte offset, and getting
that wrong is invisible in ASCII and misplaces every range the moment a label
contains a non-ASCII character.

## Design rules

Three constraints recur, and each exists so the libraries stay usable outside the
CLI — in a browser, in an editor, in someone else's tool.

**Libraries never sleep.** `Speed` *describes* pacing — how many micro-steps, then
how long to pause — and never waits. A terminal blocks a thread; a browser uses
`setTimeout`. A library that picked one would be unusable from the other.

**Libraries never render.** The display is exposed as pixels and as a notification
hook, not as ANSI escapes. `marie` turns pixels into colour blocks; a web frontend
would turn them into a canvas.

**Diagnostics are open.** A `Code` is a namespace and a name, not a closed enum, so
a downstream crate mints its own and pushes them through the same `Diagnostic`,
`Sink`, level machinery and renderer as the built-ins.

## Testing

Roughly 400 tests, in four kinds:

- **Compatibility** — MARIE.js's own assembler and simulator tests, ported verbatim.
- **End-to-end** — assemble, then execute on the VM, and check the number. A
  divergence anywhere surfaces as a wrong answer rather than a passing unit test on
  a wrong assumption.
- **No false positives** — real, working, VM-verified programs asserted to draw zero
  lint findings. A linter that flags working code gets switched off, at which point
  it catches nothing.
- **Robustness** — pathological input, multi-byte characters, CRLF, empty files,
  and Unix signal handling driven against the real binaries.
