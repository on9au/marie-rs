# The `marie` command

```console
$ marie <asm|run|lint|debug|disasm> [options] <file>
```

Problems are reported the way a compiler reports them — source excerpt, carets
under the offending span, a trailing `help:` line — whether they come from the
assembler or a lint.

Anywhere a program is taken, it may be MARIE assembly or a `.bin` memory image.
The extension decides; `--format asm|bin` overrides it.

## Common options

| Flag | |
|---|---|
| `--format <asm\|bin\|auto>` | How to read the input. Default: `.bin` is an image, anything else is source |
| `--origin <ADDR>` | Hexadecimal address. For a **full** 8192-byte image this is the entry point, since the words can only occupy the whole address space; for a shorter fragment it is where the words load. Conflicts with an `ORG` directive |

## `marie asm`

Assemble a program.

| Flag | |
|---|---|
| `-l, --listing` | Print `address  word  source` for every line |
| `--hex` | Print the assembled words as hexadecimal, one per line |
| `--lint` | Also report lint findings |
| `-o, --output <PATH>` | Write a binary memory image |
| `--bare` | With `-o`, write only the program's words instead of the whole address space |

```console
$ marie asm program.mas --listing
000  1004          Load  First
001  3005          Add   Second
002  6000          Output
003  7000          Halt
004  0015  First,  DEC 21
005  0015  Second, DEC 21
```

`-o` writes MARIE.js's format: little-endian 16-bit words, the full 4096-word
memory unless `--bare`. See [Compatibility](compatibility.md#the-binary-image-format).

## `marie run`

Assemble and execute. `Input` reads stdin, `Output` writes stdout.

| Flag | |
|---|---|
| `--max-steps <N>` | Stop after N instructions. **Unlimited by default** |
| `--registers` | Print the registers when the program stops |
| `-d, --display` | Draw the memory-mapped display as the program runs |
| `--refresh <N>` | Instructions between display refreshes (default 3000) |
| `-s, --speed <0-9>` | Pace execution like the MARIE.js slider |
| `--speeds` | List the speed levels and exit |

The speed levels count **register transfers**, not instructions, so level 0
advances the machine one micro-operation per second — slow enough to watch the
fetch-decode-execute cycle. See [Compatibility](compatibility.md#execution-speed).

```console
$ marie run badapple.bin --display --speed 7
```

**Ctrl-C** stops the machine rather than killing the process: it reports where it
stopped and how far it got, and exits `130`. A second press exits at once.

## `marie lint`

Check for mistakes that assemble cleanly. See [Lints](lints.md).

| Flag | |
|---|---|
| `-A, --allow <CODE>` | Silence a lint. Repeatable |
| `-D, --deny <CODE>` | Treat a lint as an error. Repeatable |
| `--deny-all` | Treat every lint as an error |
| `--list` | List the available lints and exit |

Codes may be written bare or fully qualified: `--deny falls-into-data` and
`--deny lint::falls-into-data` are the same. Findings alone exit `0`; a denied
finding exits non-zero, while still leaving the program assembled.

## `marie debug`

Step through a program, forwards and backwards.

| Flag | |
|---|---|
| `--history <N>` | Micro-operations kept for stepping backwards (default 100000) |
| `-b, --break <ADDR>` | Set a breakpoint before starting. Repeatable |
| `-d, --display` | Draw the display after every command that moves the machine |

| Command | |
|---|---|
| `step`, `s` | Run one instruction |
| `stepi`, `si` | Run one micro-operation |
| `back`, `b` | **Undo** one instruction |
| `backi`, `bi` | Undo one micro-operation |
| `continue`, `c` | Run until a breakpoint, a halt, or a fault |
| `regs`, `r` | Show the registers |
| `mem`, `m ADDR` | Show sixteen words from ADDR |
| `display`, `d` | Draw the 16×16 display |
| `list`, `l` | Show the source around the program counter |
| `break`/`delete` `ADDR`, `breaks` | Manage breakpoints |
| `quit`, `q` | Leave |

```text
-> 002     4          Store Total
(marie) back
-> 001     3          Add   Second
(marie) stepi
  MAR <- PC
.. 001     3          Add   Second
```

`->` is an instruction boundary, `..` part-way through one. Reverse execution is
real, not re-simulation: the machine keeps an undo journal of micro-operations, and
the debugger's I/O device hands back input it has already consumed so `back` works
across `Input`. Debugging a `.bin` works too — with no source map, `list` falls
back to disassembling the word at the program counter.

**Ctrl-C** during `continue` returns to the prompt rather than ending the session.
At the prompt it quits.

## `marie disasm`

Read a binary image back as assembly. An image has no labels, comments or line
numbers, so this shows what each word decodes to rather than reconstructing source.

| Flag | |
|---|---|
| `--origin <ADDR>` | The address the image starts at (default `000`) |
| `-a, --all` | Print every word, including the trailing zeros |

```console
$ marie disasm program.bin
000  1004  Load 004 (4100)
001  3005  Add 005 (12293)
002  6000  Output (24576)
003  7000  Halt (28672)
004  0015  JnS 015 (21)
... 4090 zero words omitted (use --all to show)
```

The decimal value is shown alongside because a data word decodes as a nonsense
instruction, and you need both to tell which it is — `004` above is `DEC 21`.
