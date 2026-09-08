# MARIE assembly

A reference for the language this assembler accepts. It is the [MARIE.js][mariejs]
dialect; where that differs from the textbook, [Compatibility](compatibility.md)
says how and why.

## The machine

- **Words** are 16 bits, signed. Arithmetic wraps.
- **Memory** is 4096 words, addressed `000`–`FFF`. Addresses are 12 bits, so the
  operand field of an instruction is three hexadecimal digits.
- **Registers**: `AC` (accumulator), `PC`, `IR`, `MAR`, `MBR`, `IN`, `OUT`. `PC`
  and `MAR` are 12 bits; the rest are 16.
- **Display**: memory `F00`–`FFF` is a 16×16 colour display. See
  [the display](#the-display).

An instruction word is a 4-bit opcode and a 12-bit operand:

```text
 15  12 11                     0
+------+------------------------+
| op   |        operand         |
+------+------------------------+
```

## Line syntax

```text
label,  Mnemonic  operand    / comment
```

Every part is optional except the mnemonic. Concretely:

- **Comments** start with `/` and run to the end of the line.
- **Labels** end with a comma. They may not start with a digit or contain
  whitespace, and each must be unique. They are **case-sensitive**.
- **Mnemonics** are **case-insensitive**: `halt`, `Halt` and `HALT` are one
  instruction. The canonical spellings are in the tables below.
- **Operands** are a single word with no spaces.

A line that does not fit this shape is an error, not a guess: `Load X Y` is
rejected rather than silently read as `Load X`.

## Operands

> **Instruction operands are hexadecimal.** `Add 123` is address `0x123`. This is
> the single most common source of confusion in MARIE.

An operand is a **literal** if it matches `/^\d[0-9a-fA-F]*$/` — that is, if it
starts with a *decimal* digit. Otherwise it is a **label reference**.

| Written | Read as |
|---|---|
| `Load 1A` | address `0x01A` |
| `Load A1` | a reference to a label named `A1` |
| `Load 0A1` | address `0x0A1` |

Leading zeros carry no weight, so `Add 00000FFF` is `Add FFF`. An operand above
`0xFFF` does not fit the address field and is an error.

The rule catches people out most often with `Skipcond`:

```text
        Skipcond C00        / error: `C00` is a label reference
        Skipcond 0C00       / correct: the leading zero makes it a literal
```

## Instructions

| Opcode | Mnemonic | Effect |
|---|---|---|
| `0x0` | `JnS X` | Store the address of the next instruction at X, then jump to X + 1 |
| `0x1` | `Load X` | Load the value at address X into the AC (AC ← M[X]) |
| `0x2` | `Store X` | Store the AC into memory at address X (M[X] ← AC) |
| `0x3` | `Add X` | Add the value at address X to the AC (AC ← AC + M[X]) |
| `0x4` | `Subt X` | Subtract the value at address X from the AC (AC ← AC − M[X]) |
| `0x5` | `Input` | Read the next value from user input (AC ← IN) |
| `0x6` | `Output` | Output the value in the AC (OUT ← AC) |
| `0x7` | `Halt` | Stop execution |
| `0x8` | `Skipcond X` | Skip the next instruction if the condition X holds |
| `0x9` | `Jump X` | Jump to address X (PC ← X) |
| `0xA` | `LoadImmi X` | Set the AC to the 12-bit unsigned immediate X (AC ← X) |
| `0xB` | `AddI X` | Add the value at the address held at X (AC ← AC + M[M[X]]) |
| `0xC` | `JumpI X` | Jump to the address held at X (PC ← M[X]) |
| `0xD` | `LoadI X` | Load the value at the address held at X (AC ← M[M[X]]) |
| `0xE` | `StoreI X` | Store the AC at the address held at X (M[M[X]] ← AC) |

Opcode `0xF` is unassigned; executing it stops the machine with a fault.

`Input`, `Output` and `Halt` take no operand, and giving them one is an error.

### Skipcond

Only bits 11–10 of the operand select the condition. The rest are **ignored**, so
`Skipcond 100` is not a fifth condition and not an error — it silently behaves as
`Skipcond 000`. The [`masked-skipcond`](lints.md#lintmasked-skipcond) lint exists for
exactly this.

| Operand | Skips if |
|---|---|
| `000` | AC < 0 |
| `400` | AC = 0 |
| `800` | AC > 0 |
| `0C00` | AC ≠ 0 |

### JnS and subroutines

`JnS X` writes the return address into `M[X]` and continues at `X + 1`, so `X`
must be a spare word — not an instruction, which the call would overwrite. The
usual shape is:

```text
        JnS   Double        / call
        Store Result
        Halt

Double, HEX 0               / the return address lands here
        Add   Value
        JumpI Double        / return
```

## Directives

| Directive | Effect |
|---|---|
| `ORG X` | Assemble the following code at address X |
| `DEC n` | Emit the decimal literal n |
| `OCT n` | Emit the octal literal n |
| `HEX n` | Emit the hexadecimal literal n |
| `ADR X` | Emit the address X as a word |
| `Clear` | Set the AC to zero (an alias for `LoadImmi 0`) |
| `END` | Stop assembling |

### Literal ranges

`DEC` accepts `-32768` to `65535` and stores the bit pattern, so `DEC -1` and
`DEC 65535` both produce `0xFFFF`. `OCT` and `HEX` are unsigned, `0` to `0xFFFF`,
and may **not** carry a sign. A leading `+` is rejected in every base.

### ORG

`ORG` takes exactly **three hexadecimal digits**, must come before any word, and
may appear once. Anything else is not an origin directive at all — `ORG 10` and
`ORG 1000` are parsed as ordinary statements and reported as an unknown mnemonic.

### END

`END` stops assembly. Nothing after it is parsed, so errors below it are not
reported and labels defined there do not exist.

## The display

Memory `F00`–`FFF` is a 16×16 display, one word per pixel, row-major:
`F00 + 16 × row + column`. Each word is a colour, five bits per channel:

```text
 15  14      10 9       5 4       0
+---+----------+---------+---------+
| - |    R     |    G    |    B    |
+---+----------+---------+---------+
```

Bit 15 is unused. Channels widen to eight bits by scaling — `round(c / 31 × 255)`,
so full brightness is 255 — not by shifting. Drawing is just storing:

```text
        Load  Red
        Store 0F00          / top-left pixel
        Halt
Red,    HEX 7C00
```

`marie run --display` draws it in the terminal.

## A complete program

```text
/ Sum inputs until a zero is entered, then print the total.
        Clear
        Store Total
Loop,   Input
        Skipcond 400        / stop when the value is zero
        Jump  Accumulate
        Load  Total
        Output
        Halt
Accumulate, Add Total
        Store Total
        Jump  Loop
Total,  DEC 0
```

[mariejs]: https://marie.js.org
