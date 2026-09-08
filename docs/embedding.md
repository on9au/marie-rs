# Embedding

Using the libraries directly. `cargo doc --workspace --open` has the full API; this
covers the entry points and the shapes that are not obvious from the types.

## Assembling

```rust
use mrs_asm::assemble;

let assembly = assemble("        Load  X\n        Halt\nX, DEC 21\n");
assert!(assembly.succeeded());
assert_eq!(assembly.words[0] as u16, 0x1002);
assert_eq!(assembly.symbols.address_of("X").unwrap().value(), 0x002);
```

`Assembly` also carries the syntax tree, the symbol table with reference sites, a
two-way source map, and the diagnostics. It is returned whether or not assembly
succeeded, so an editor can use the parts that worked.

## Running

```rust
use mrs_vm::{MarieVM, io::VecIo, states::RunOutcome};

let mut vm = MarieVM::new(VecIo::new([37]));
let (origin, words) = assembly.program_image();
vm.load_program(origin, words)?;

let RunOutcome::Terminated(vm) = vm.boot().run() else { panic!("should halt") };
assert_eq!(vm.io().outputs, vec![42]);
```

`run_bounded` takes a step budget. Stepping needs the `Stepping` state, from
`debug()` rather than `boot()`; set a history limit first if you want `step_back`.

## Writing a lint

The lint set is open. Mint codes in your own namespace and register them alongside
the built-ins:

```rust
use mrs_asm::diagnostic::{Code, Diagnostic, Sink};
use mrs_asm::lint::{Lint, LintContext};
use mrs_lint::{Level, Linter};

const NO_INPUT: Code = Code::new("housestyle", "no-input");

struct RequiresInput;

impl Lint for RequiresInput {
    fn code(&self) -> Code { NO_INPUT }
    fn description(&self) -> &'static str { "the program never reads input" }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let reads = cx.assembly.program.items.iter()
            .any(|item| item.mnemonic.opcode() == Some(mrs_core::Opcode::Input));
        if !reads && let Some(first) = cx.assembly.program.items.first() {
            out.push(Diagnostic::warning(
                self.code(), first.span, first.line,
                "This program never reads input.",
            ));
        }
    }
}

let outcome = Linter::new().with(&RequiresInput).deny(NO_INPUT).check(source);
```

A lint sees a finished assembly — resolved addresses, reference counts, emitted
words — and must not change the output. If it would change the program, it is an
error, not a lint. `mrs_lint::graph` builds the control-flow graph for lints that
need one, and returns `None` when the program did not assemble.

## Diagnostics

`Sink` is the destination, so diagnostics can be converted as they arrive rather
than collected:

```rust
use mrs_asm::diagnostic::{Diagnostic, Filtered, FromFn};

// Stream them.
let mut sink = FromFn(|d: Diagnostic| eprintln!("{d}"));
let assembly = mrs_asm::assemble_into(source, &mut sink);

// Or apply an allow-list.
let mut kept: Vec<Diagnostic> = Vec::new();
let mut sink = Filtered::new(&mut kept, |d: &Diagnostic| d.code != Code::UNUSED_LABEL);
mrs_asm::assemble_into(source, &mut sink);
```

With the `pretty` feature (on by default), `Assembly::report` renders a
compiler-style report through [miette].

## Driving a display

The display is the top of memory, so it can be read at any time — but a frontend
usually wants to be told. The hook is on the I/O device, which means a frontend
*is* a device:

```rust
use mrs_core::display::Rgb555;
use mrs_vm::io::MarieVmIODevice;

impl MarieVmIODevice for Frontend {
    // ...poll_input and output...

    fn display_write(&mut self, index: usize, pixel: Rgb555) {
        self.updates.push((index, pixel.to_rgb8()));
    }
}
```

`index` is row-major over 256 pixels. It fires only when a pixel actually changes,
and also when `step_back` restores one — so a frontend drawing from the hook stays
correct while the debugger rewinds. `vm.display()` gives a full view for a frontend
that would rather snapshot per frame: `pixel(col, row)`, `rows()`, `to_rgb8()`.

## Pacing

`Speed` describes pacing and never sleeps, so the same levels drive a terminal and a
browser:

```rust
use mrs_vm::speed::Speed;

let speed = Speed::new(3).unwrap();
let batch = speed.micro_steps();          // Some(1), or None for unlimited
let delay = speed.delay();                // how long to wait afterwards
// Also cut the batch short at Speed::BATCH_TIME_LIMIT, whatever the level.
```

## Language-server pieces

`mrs-lsp` exposes each feature as a function over an analysed `Document`, so a
different protocol layer can reuse them. `PositionEncoding` handles the byte-offset
to LSP-column conversion; do not open-code it.

[miette]: https://docs.rs/miette
