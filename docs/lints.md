# Lints

The assembler already rejects everything that is not valid MARIE, so these are
about programs that assemble perfectly and are still wrong. Most such mistakes are
invisible to a single line and only show up once you know where control can go.

```console
$ marie lint program.mas
$ marie lint program.mas --deny falls-into-data --allow non-canonical-mnemonic
$ marie lint --list
```

Every lint has a stable code in a namespace: `lint::` for this catalogue, `asm::`
for the three that describe quirks of assembly rather than of programs. Codes are
stable across releases; the wording is not.

Levels are `allow`, `warn` (the default) and `deny`. A denied finding fails the run
while still leaving the program assembled.

## Control flow

These need a control-flow graph over the assembled words.

> **On soundness.** `JumpI` and `StoreI` mean a static graph *under*-approximates
> where control can go. So "this word **is** reachable" is safe — it comes from
> following a concrete path, and missing edges cannot make it wrong — while "this
> word is **not** reachable" is only sound when the program has no indirect
> transfers at all. The unreachable-code lint stays silent when it does.

### `lint::falls-into-data`

Execution can reach a word declared with `DEC`, `OCT`, `HEX` or `ADR`. Almost
always a missing `Halt` between the last instruction and the variables below it.

The finding names what the data would be executed as, which is what makes it
concrete: `DEC 21` is `0x0015`, which decodes as `JnS 015`.

```text
        Load  X
        Add   X
X,      DEC 21          / reached by execution
```

### `lint::falls-off-end`

Execution can run past the last assembled word. MARIE keeps fetching whatever is
there — usually zeros, which decode as `JnS 000` and scribble over address zero.

### `lint::unreachable-instruction`

A word no path from the entry point can reach. Only reported when the program has
no indirect control transfers. Data is never reported: unreachable data is a
variable.

### `lint::jumps-outside-program`

A jump targets an address the program does not occupy. That memory is zero-filled,
so execution would run into `JnS 000`.

## Hazards

Constructs that assemble cleanly and then do something other than what they look
like.

### `lint::masked-skipcond`

A `Skipcond` operand with bits that are silently ignored. Only bits 11–10 select
the condition, so `Skipcond 100` is not a fifth condition and not an error — it
behaves as `Skipcond 000` and tests `AC < 0`.

This is the one MARIE mistake that produces a working program that tests the wrong
thing.

### `lint::skipcond-label-operand`

A `Skipcond` whose operand is a label. The operand is a condition selector, not an
address, so the label's address is reinterpreted as whichever condition bits 11–10
of it happen to name. Use `Jump` to branch.

### `lint::jns-overwrites-code`

`JnS X` writes the return address into `M[X]`, so `X` must be a spare word.
Pointing a call at code destroys that instruction the first time it runs. Reserve a
slot: `Sub, HEX 0`.

### `lint::self-modifying-code`

A `Store` that writes over a word which is executed as an instruction. Legal and
occasionally deliberate, so this is *advice* rather than a warning — but worth
being sure it was meant.

## Style

### `lint::missing-halt`

The program contains no `Halt` anywhere.

### `lint::non-canonical-mnemonic`

A mnemonic not in its canonical spelling — `HALT` rather than `Halt`. Mnemonics are
case-insensitive, so this changes nothing; consistent spelling is just easier to
search. The language server offers a fix.

### `lint::label-shadows-mnemonic`

A label spelled like an instruction or directive. Legal — labels and mnemonics are
separate namespaces — but `Load, Load Load` is a sentence nobody should have to
parse.

## Assembly quirks

These live in the `asm::` namespace because they describe the assembler's own
behaviour rather than a program's logic. They ship with `mrs-asm` and are included
in the default lint set.

### `asm::ignored-operand`

An operand the assembler silently discards. `Clear` always assembles as
`LoadImmi 0`, and MARIE.js overwrites whatever operand was written rather than
rejecting it — so `Clear 5` quietly loads zero.

### `asm::unused-label`

A label nothing refers to.

### `asm::unreachable-code`

Text after an `END` directive. Nothing there is parsed, so it is not merely
unreachable at run time — it does not exist.

## Adding your own

The lint set is open. A downstream crate implements `mrs_asm::lint::Lint`, mints
codes in its own namespace, and registers them alongside the built-ins — the
diagnostics flow through the same renderer, the same level machinery and the same
language server. See [Embedding](embedding.md#writing-a-lint).
