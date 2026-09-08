//! `marie debug` — step through a program interactively.
//!
//! This is the only place the VM's reverse execution is driven by a person, so the
//! session owns two things the other subcommands do not: a history limit, and an I/O
//! device that can give back what it has already handed over. Without the latter,
//! stepping backwards over an `Input` reports the operation as irreversible and stops.

use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::task::Poll;

use clap::Args as ClapArgs;
use mrs_asm::Assembly;
use mrs_core::MemoryAddress;
use mrs_vm::MarieVM;
use mrs_vm::history::StepBackError;
use mrs_vm::io::{IoError, MarieVmIODevice};
use mrs_vm::states::{MicroStepOutcome, StepOutcome, Stepping, Terminated};

use crate::input::{self, Load};
use crate::interrupt::Interrupt;
use crate::name;

/// Arguments to `marie debug`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The program to debug: assembly, or a `.bin` memory image.
    pub file: PathBuf,

    #[command(flatten)]
    pub load: Load,

    /// How many micro-operations to keep for stepping backwards.
    #[arg(long, default_value_t = 100_000)]
    pub history: usize,

    /// Set a breakpoint at a hexadecimal address before starting. May be repeated.
    #[arg(short, long = "break", value_name = "ADDR")]
    pub breakpoints: Vec<String>,

    /// Draw the memory-mapped display after every command that moves the machine.
    #[arg(short, long)]
    pub display: bool,
}

/// An input device that can be rewound.
///
/// [`StdinIo`](mrs_vm::io::StdinIo) reads straight from the terminal and cannot give a
/// value back, which would make `back` fail the moment it crossed an `Input`. This
/// keeps what it has read so the debugger can hand it over again.
#[derive(Debug, Default)]
struct ReplayIo {
    /// Values read from the terminal and then pushed back by a rewind.
    pending: VecDeque<i16>,
    /// Everything written, so an output can be retracted.
    outputs: Vec<i16>,
}

impl MarieVmIODevice for ReplayIo {
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        if let Some(value) = self.pending.pop_front() {
            println!("input> {value} (replayed)");
            return Poll::Ready(Ok(value));
        }
        let stdin = std::io::stdin();
        loop {
            print!("input> ");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Err(error) => return Poll::Ready(Err(IoError::Io(error))),
                Ok(0) => return Poll::Ready(Err(IoError::Eof)),
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match mrs_core::literal::parse_prefixed_word(trimmed) {
                        Ok(word) => return Poll::Ready(Ok(word.value())),
                        Err(error) => eprintln!("{error}"),
                    }
                }
            }
        }
    }

    fn output(&mut self, value: i16) -> Result<(), IoError> {
        println!("output> {value}");
        self.outputs.push(value);
        Ok(())
    }

    fn unread_input(&mut self, value: i16) -> bool {
        self.pending.push_front(value);
        true
    }

    fn unwrite_output(&mut self, value: i16) -> bool {
        // The text is already on the terminal and cannot be taken back, but the record
        // can be, which is what keeps a replay consistent.
        if self.outputs.last() == Some(&value) {
            self.outputs.pop();
            println!("(undid output {value})");
            true
        } else {
            false
        }
    }
}

/// The machine, which changes type as it runs.
///
/// Every transition consumes the VM, so the session holds it in an `Option` and moves
/// it out for the duration of a command rather than keeping a placeholder around.
enum Machine {
    Stepping(Box<MarieVM<ReplayIo, Stepping>>),
    Terminated(Box<MarieVM<ReplayIo, Terminated>>),
}

/// Starts an interactive session.
pub fn run(args: Args) -> miette::Result<()> {
    let program = input::load(&args.file, &args.load)?;

    let mut vm = MarieVM::new(ReplayIo::default());
    program.install(&mut vm)?;
    vm.set_history_limit(args.history);

    for written in &args.breakpoints {
        let address = parse_address(written)?;
        vm.breakpoints_mut().insert(address);
    }

    let mut machine = Some(Machine::Stepping(Box::new(vm.debug())));
    println!(
        "{} — {} words at {}. Type `help` for commands.",
        name(&args.file),
        program.words.len(),
        program.entry
    );
    if !program.has_source() {
        // An image carries no line numbers, so `list` falls back to disassembly.
        println!("(binary image: showing disassembly instead of source)");
    }
    // Disarmed at the prompt, so Ctrl-C there quits the debugger; armed only around
    // `continue`, where it means "stop the machine and come back".
    let context = Context {
        assembly: program.assembly.as_ref(),
        source: program.source.as_deref(),
        follow_display: args.display,
        stop: Interrupt::install_disarmed(),
    };
    show_location(&machine, &context);

    let stdin = std::io::stdin();
    loop {
        print!("(marie) ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if matches!(stdin.lock().read_line(&mut line), Ok(0) | Err(_)) {
            return Ok(());
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match command(line, &mut machine, &context) {
            Flow::Continue => {}
            Flow::Quit => return Ok(()),
        }
    }
}

/// Whether the session keeps going.
enum Flow {
    Continue,
    Quit,
}

/// Runs one debugger command.
fn command(line: &str, machine: &mut Option<Machine>, context: &Context<'_>) -> Flow {
    let mut parts = line.split_whitespace();
    let verb = parts.next().unwrap_or_default();
    let argument = parts.next();

    match verb {
        "help" | "h" | "?" => print_help(),
        "quit" | "q" => return Flow::Quit,

        "step" | "s" => step(machine, context),
        "stepi" | "si" => micro_step(machine, context),
        "back" | "b" => step_back(machine, context, false),
        "backi" | "bi" => step_back(machine, context, true),
        "continue" | "c" => resume(machine, context),

        "regs" | "r" => match machine.as_ref() {
            Some(Machine::Stepping(vm)) => println!("{}", vm.registers()),
            Some(Machine::Terminated(vm)) => println!("{}", vm.registers()),
            None => {}
        },
        "mem" | "m" => memory(machine, argument),
        "display" | "d" => show_display(machine),
        "list" | "l" => show_location(machine, context),
        "break" => breakpoint(machine, argument, true),
        "delete" => breakpoint(machine, argument, false),
        "breaks" => breakpoints(machine),

        other => println!("unknown command `{other}` — try `help`"),
    }
    Flow::Continue
}

/// Prints the command list.
fn print_help() {
    println!(
        "\
  step, s        run one instruction
  stepi, si      run one micro-operation
  back, b        undo one instruction
  backi, bi      undo one micro-operation
  continue, c    run until a breakpoint, a halt, or a fault
  regs, r        show the registers
  mem, m ADDR    show sixteen words from ADDR (hexadecimal)
  display, d     draw the 16x16 memory-mapped display
  list, l        show the source around the program counter
  break ADDR     set a breakpoint
  delete ADDR    remove a breakpoint
  breaks         list the breakpoints
  help, h        this list
  quit, q        leave"
    );
}

/// Executes one instruction.
fn step(machine: &mut Option<Machine>, context: &Context<'_>) {
    match machine.take() {
        Some(Machine::Stepping(vm)) => {
            *machine = Some(match vm.step() {
                StepOutcome::Stepped(vm) => Machine::Stepping(Box::new(vm)),
                StepOutcome::AwaitingInput(vm) => {
                    println!("waiting for input");
                    Machine::Stepping(Box::new(vm))
                }
                StepOutcome::Terminated(vm) => {
                    println!("halted");
                    Machine::Terminated(Box::new(vm))
                }
                StepOutcome::Faulted(vm, fault) => {
                    println!("fault: {fault}");
                    Machine::Terminated(Box::new(vm))
                }
            });
            show_location(machine, context);
        }
        other => {
            println!("the program has stopped; `back` still works");
            *machine = other;
        }
    }
}

/// Executes one micro-operation.
fn micro_step(machine: &mut Option<Machine>, context: &Context<'_>) {
    match machine.take() {
        Some(Machine::Stepping(vm)) => {
            *machine = Some(match vm.micro_step() {
                MicroStepOutcome::Stepped(vm, operation) => {
                    println!("  {operation}");
                    Machine::Stepping(Box::new(vm))
                }
                MicroStepOutcome::AwaitingInput(vm) => {
                    println!("waiting for input");
                    Machine::Stepping(Box::new(vm))
                }
                MicroStepOutcome::Terminated(vm) => {
                    println!("halted");
                    Machine::Terminated(Box::new(vm))
                }
                MicroStepOutcome::Faulted(vm, fault) => {
                    println!("fault: {fault}");
                    Machine::Terminated(Box::new(vm))
                }
            });
            show_location(machine, context);
        }
        other => {
            println!("the program has stopped; `back` still works");
            *machine = other;
        }
    }
}

/// Reverses execution.
fn step_back(machine: &mut Option<Machine>, context: &Context<'_>, micro: bool) {
    // A stopped machine can be handed back to the debugger, which is what makes it
    // possible to rewind out of a halt. Every arm must put the machine back: taking it
    // and dropping it on a non-matching pattern would empty the slot for good.
    match machine.take() {
        Some(Machine::Terminated(vm)) => {
            *machine = Some(Machine::Stepping(Box::new(vm.debug())));
        }
        other => *machine = other,
    }
    let Some(Machine::Stepping(vm)) = machine.as_mut() else {
        return;
    };

    let result = if micro {
        vm.micro_step_back().map(|operation| {
            println!("  undid {operation}");
            1
        })
    } else {
        vm.step_back()
    };
    match result {
        Ok(_) => show_location(machine, context),
        Err(StepBackError::NoHistory) => {
            println!("nothing recorded to undo — raise --history to rewind further")
        }
        Err(error) => println!("{error}"),
    }
}

/// Runs until something stops the machine.
fn resume(machine: &mut Option<Machine>, context: &Context<'_>) {
    use mrs_vm::states::{RunOutcome, SuspendReason};

    // Running in slices is what lets Ctrl-C get a word in: `run` is a tight loop inside
    // the VM with no way to look up.
    const SLICE: u64 = 200_000;

    let Some(Machine::Stepping(vm)) = machine.take() else {
        println!("the program has stopped");
        return;
    };
    let armed = context.stop.arm();
    let mut running = vm.resume();

    loop {
        if armed.requested() {
            println!("interrupted");
            *machine = Some(Machine::Stepping(Box::new(running.pause())));
            break;
        }
        match running.run_bounded(SLICE) {
            RunOutcome::Suspended(vm, SuspendReason::StepLimit) => running = vm,
            RunOutcome::Suspended(vm, reason) => {
                println!("{reason}");
                *machine = Some(Machine::Stepping(Box::new(vm.pause())));
                break;
            }
            RunOutcome::Terminated(vm) => {
                println!("halted");
                *machine = Some(Machine::Terminated(Box::new(vm)));
                break;
            }
            RunOutcome::Faulted(vm, fault) => {
                println!("fault: {fault}");
                *machine = Some(Machine::Terminated(Box::new(vm)));
                break;
            }
        }
    }
    drop(armed);
    show_location(machine, context);
}

/// Prints sixteen words from an address.
fn memory(machine: &Option<Machine>, argument: Option<&str>) {
    let start = match argument.map(parse_address) {
        Some(Ok(address)) => address,
        Some(Err(error)) => {
            println!("{error}");
            return;
        }
        None => MemoryAddress::ZERO,
    };
    let read = |address: MemoryAddress| match machine.as_ref() {
        Some(Machine::Stepping(vm)) => vm.memory().read(address),
        Some(Machine::Terminated(vm)) => vm.memory().read(address),
        None => 0,
    };
    for row in 0..4 {
        let base = start.wrapping_add(row * 4);
        let words: Vec<String> = (0..4)
            .map(|column| format!("{:04X}", read(base.wrapping_add(column)) as u16))
            .collect();
        println!("{base}  {}", words.join(" "));
    }
}

/// Adds or removes a breakpoint.
fn breakpoint(machine: &mut Option<Machine>, argument: Option<&str>, insert: bool) {
    let Some(written) = argument else {
        println!("expected an address");
        return;
    };
    let address = match parse_address(written) {
        Ok(address) => address,
        Err(error) => {
            println!("{error}");
            return;
        }
    };
    let breakpoints = match machine.as_mut() {
        Some(Machine::Stepping(vm)) => vm.breakpoints_mut(),
        Some(Machine::Terminated(vm)) => vm.breakpoints_mut(),
        None => return,
    };
    if insert {
        breakpoints.insert(address);
        println!("breakpoint at {address}");
    } else if breakpoints.remove(address) {
        println!("removed breakpoint at {address}");
    } else {
        println!("no breakpoint at {address}");
    }
}

/// Lists the breakpoints.
fn breakpoints(machine: &Option<Machine>) {
    let set = match machine.as_ref() {
        Some(Machine::Stepping(vm)) => vm.breakpoints(),
        Some(Machine::Terminated(vm)) => vm.breakpoints(),
        None => return,
    };
    if set.is_empty() {
        println!("no breakpoints");
        return;
    }
    for address in set.iter() {
        println!("{address}");
    }
}

/// What the debugger knows about the program beyond its words.
///
/// A `.bin` image carries neither: there is no source text and no source map, so the
/// listing falls back to disassembling whatever the machine currently holds.
pub struct Context<'a> {
    /// The assembly, when the program was built from source.
    pub assembly: Option<&'a Assembly>,
    /// The source text, when there was any.
    pub source: Option<&'a str>,
    /// Whether to redraw the display after every command that moves the machine.
    pub follow_display: bool,
    /// The Ctrl-C handler, armed only while the machine is running.
    pub stop: Interrupt,
}

/// Draws the memory-mapped display.
fn show_display(machine: &Option<Machine>) {
    let rendered = match machine.as_ref() {
        Some(Machine::Stepping(vm)) => crate::display::render(&vm.display()),
        Some(Machine::Terminated(vm)) => crate::display::render(&vm.display()),
        None => return,
    };
    print!("{rendered}");
}

/// Prints where the machine is, with the source line it came from.
fn show_location(machine: &Option<Machine>, context: &Context<'_>) {
    let (pc, boundary) = match machine.as_ref() {
        Some(Machine::Stepping(vm)) => (vm.registers().pc, vm.at_instruction_boundary()),
        Some(Machine::Terminated(vm)) => (vm.registers().pc, vm.at_instruction_boundary()),
        None => return,
    };

    if context.follow_display {
        show_display(machine);
    }

    let marker = if boundary { "->" } else { ".." };

    // With source, the source map turns an address back into the line a person wrote.
    if let (Some(assembly), Some(source)) = (context.assembly, context.source)
        && let Some(line) = assembly.source_map.line_for(pc)
    {
        let text = assembly
            .lines
            .line_span(line)
            .and_then(|span| span.text(source))
            .unwrap_or_default();
        println!("{marker} {pc}  {:>4}  {}", line + 1, text.trim_end());
        return;
    }

    // Without it — a binary image, or an address outside the program — disassemble the
    // word that is actually there.
    let word = match machine.as_ref() {
        Some(Machine::Stepping(vm)) => vm.memory().read(pc),
        Some(Machine::Terminated(vm)) => vm.memory().read(pc),
        None => return,
    };
    println!("{marker} {pc}  {}", crate::disasm::describe(word));
}

/// Parses a hexadecimal address written on the command line.
fn parse_address(written: &str) -> miette::Result<MemoryAddress> {
    let trimmed = written.trim_start_matches("0x").trim_start_matches("0X");
    let value = u16::from_str_radix(trimmed, 16)
        .map_err(|_| miette::miette!("'{written}' is not a hexadecimal address"))?;
    MemoryAddress::try_new(value)
        .ok_or_else(|| miette::miette!("address {written} is outside the 12-bit address space"))
}
