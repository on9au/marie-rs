# MARIE.js compatibility

This implements the [MARIE.js][mariejs] dialect, not the textbook one, and aims to
be **bug-compatible** with it: where MARIE.js does something surprising, so does
this, because a program that runs on the website should run here and compute the
same thing.

Everything below was read out of the MARIE.js source rather than inferred, and the
behaviours are pinned by tests — including MARIE.js's own assembler and simulator
tests, ported directly into `crates/lib/mrs-asm/tests/`.

## Differences from the textbook

These are the three that matter, and they are the reason a textbook MARIE emulator
will not run MARIE.js programs correctly:

| | Textbook | MARIE.js and this |
|---|---|---|
| Opcode `0xA` | `Clear` | `LoadImmi X` — load a 12-bit unsigned immediate. `Clear` is an assembler alias for `LoadImmi 0` |
| `Skipcond C00` | undefined | skip if AC ≠ 0 |
| `JnS` | clobbers the accumulator | leaves it alone |

`JnS`'s micro-program is `MAR←IR, MBR←PC, write, PC←MAR, PC←PC+1` — no accumulator
anywhere — which is what makes the ordinary subroutine idiom work.

## The assembler

### Operands are hexadecimal

`Add 123` assembles to `0x3123`. Only `DEC`, `OCT` and `HEX` literals use another
base, and the directive nearby makes no difference to an instruction's operand.

### A leading decimal digit makes an operand a literal

MARIE.js decides with `/^\d[0-9a-fA-F]*$/`. `Load 1A` is address `0x1A`; `Load A1`
is a label reference. The consequence people hit is that the non-zero skip
condition cannot be written `Skipcond C00` — `C00` starts with a letter, so it is
read as a label and the program fails to assemble with `Unknown label 'C00'`.

It has to be `Skipcond 0C00`. This is also why MARIE.js's own documentation writes
that one condition as `0C00` while the other three are bare `000`, `400` and `800`.

### Labels are case-sensitive, mnemonics are not

`Foo` and `foo` are two different labels. `halt`, `Halt` and `HALT` are one
instruction. Getting this backwards is a silent source of divergence, so the two
are kept deliberately far apart in the implementation.

### ORG is strict

`/^\s*org\s+([0-9a-f]{3})\s*(?:\/.*)?$/i` — exactly three hexadecimal digits, at
most once, before any emitted word. `ORG 10`, `ORG 1000` and `ORG100` are not
origin directives; they fall through to the statement form and are reported as an
unknown mnemonic. An `ORG` that arrives too late is reported and *ignored*, leaving
the origin as it was.

### A line with an invalid label is skipped whole

If a label starts with a digit, contains whitespace, or is a duplicate, MARIE.js
reports it and moves on **without emitting a word** — so every address after it
shifts down by one. Reproduced, because the addresses of everything below depend
on it.

### Literal ranges and signs

`DEC` accepts `-0x8000..=0xFFFF` and masks; other bases are unsigned `0..=0xFFFF`.
Signs are permitted only in decimal, because MARIE.js matches decimal with
`/^-?\d+$/` and the other bases with patterns that have no sign at all. A leading
`+` is therefore a parse failure in every base — `parse_word` in `mrs-core` rejects
it for that reason.

### Quirks reproduced deliberately

| Behaviour | Why it looks wrong |
|---|---|
| `Clear 5` assembles as `LoadImmi 0` | MARIE.js overwrites the operand instead of rejecting it. The [`ignored-operand`](lints.md#asmignored-operand) lint flags it without changing the output |
| `Skipcond 100` behaves as `Skipcond 000` | Only bits 11–10 select the condition; the rest are masked away silently. See [`masked-skipcond`](lints.md#lintmasked-skipcond) |
| A label on an `END` line is still defined | The label is recorded before the `END` is noticed, so it points one word past the program |

## The binary image format

MARIE.js's *Download .bin* writes the whole address space:

```js
const array = new ArrayBuffer(sim.memory.length * 2);
const view = new DataView(array);
for (let i = 0; i < sim.memory.length; i++) {
    view.setInt16(i * 2, sim.memory[i], true);
}
```

- **Little-endian.** `DataView.setInt16` defaults to *big*-endian; the trailing
  `true` selects little. A reader that took the default would load every word
  byte-swapped — `Load 004` (`0x1004`) becoming `0x0410`, a jump to the wrong
  address with nothing to signal it.
- **Signed 16-bit words.**
- **8192 bytes**, the entire 4096-word memory, not just the program.

MARIE.js only writes this format — its file picker accepts `.mas` and `.mar` — so
reading one back is an addition. `marie` also accepts a shorter file as a fragment
to load at `--origin`.

## The display

`0xF00`–`0xFFF`, 16×16, one word per pixel, indexed `0xF00 + 16 × row + column`,
with `R[14-10] G[9-5] B[4-0]`. Channels widen by **scaling**, not shifting:

```js
const b = Math.round(((x & 0x1f) / 31.0) * 255.0);
```

`c << 3` is the usual shortcut and is wrong here: it maps full brightness to 248,
so white comes out slightly grey and no pixel ever reaches the top of the range.

## Input

MARIE.js's "Inputs" panel is given a whole string and an input mode, and each
`Input` the program executes takes the next value from it. `marie run --input`
spells the same choice: `word` — the default — is a value per line, while `utf16`
decodes the line to UTF-16 and hands over one code unit per `Input`, which is the
panel's Unicode (UTF-16BE) mode. So `123 + 456 =` feeds eleven `Input`
instructions in both.

A MARIE word holds a whole code unit, so the byte order the name promises never
actually shows. JavaScript strings are sequences of UTF-16 code units natively, so
both implementations spend two `Input`s on a character outside the Basic
Multilingual Plane.

## Execution speed

The ten-position slider paces **micro-operations**, not instructions: MARIE.js
paces `doRunStep`, which micro-steps and skips its `decode` and `step-end` marker
actions. That leaves `5 + execute` paced units per instruction — exactly the number
of `MicroOp`s this crate executes, since its `Decode` sits where MARIE.js has its
`step` marker. So a level advances the machine at the same rate in both.

| Level | Micro-steps | Delay | | Level | Micro-steps | Delay |
|---|---|---|---|---|---|---|
| 0 | 1 | 1000 ms | | 5 | 1 | 0 |
| 1 | 1 | 500 ms | | 6 | 10 | 0 |
| 2 | 1 | 250 ms | | 7 | 50 | 0 |
| 3 | 1 | 10 ms | | 8 | 100 | 0 |
| 4 | 1 | 30 ms | | 9 | ∞ | 0 |

A batch is also cut short after 20 ms whatever the level, so a fast level cannot
starve whatever is drawing.

> **Levels 3 and 4 are transposed in MARIE.js.** Level 3 delays 10 ms and level 4
> delays 30 ms, so sliding right from 3 to 4 makes the program *slower*. The
> sequence reads `1000, 500, 250, 10, 30, 0`; almost certainly a typo for
> `…250, 30, 10, 0`. Reproduced as-is so `--speed 4` matches the website, and
> `Speed::is_monotonic()` returns `false` so a caller that would rather present a
> sensible slider can tell.

## Micro-operation counts

MARIE.js brackets each cycle with two marker pseudo-steps — a leading `step` and a
trailing `step-end` — that perform no register transfer. This crate has neither,
and has `Decode` as a real operation where MARIE.js emits a `decode` action.

The **sequence of register transfers is identical**; only the markers differ. `Load
X` is 10 `microStep()` calls in MARIE.js and 8 `MicroOp`s here. Input looks
different again — MARIE.js models the stall with an interrupt state machine outside
the micro-program — but the observable changes are `IN ← value` then `AC ← IN` in
both.

This matters only if you are comparing micro-step indices with the website's
animation, never to a program's results.

## The one deliberate deviation

A program whose words run past the end of memory is rejected **at assembly time**
here, with `asm::program-too-large` naming the offending line.

MARIE.js assigns out-of-range addresses without complaint and only refuses the
program when the simulator loads it — by which point any instruction referring to
such an address has a corrupted opcode field, since the address overflows into it.

No program that MARIE.js could actually run is rejected by this: it cannot load
those either. The error is simply raised where it can point at a line.

## Verifying it yourself

```console
$ cargo test --workspace
```

`crates/lib/mrs-asm/tests/compat.rs` ports MARIE.js's assembler tests verbatim, and
`execution.rs` ports its simulator tests and runs them on this VM — so a divergence
in operand base, label address, opcode encoding or micro-program surfaces as a
wrong number rather than as a passing test on a wrong assumption.

[mariejs]: https://marie.js.org
