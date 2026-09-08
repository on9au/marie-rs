//! `marie run` — assemble a program and execute it.

use std::path::PathBuf;

use std::time::Instant;

use clap::Args as ClapArgs;
use mrs_vm::speed::Speed;
use mrs_vm::states::MicroStepOutcome;
use mrs_vm::{MarieVM, io::StdinIo, states::RunOutcome};

use crate::display;
use crate::input::{self, Load};
use crate::interrupt::{self, Interrupt};
use crate::stdin::InputMode;

/// Arguments to `marie run`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The program to run: assembly, or a `.bin` memory image. Not needed with
    /// `--speeds`.
    #[arg(required_unless_present = "speeds")]
    pub file: Option<PathBuf>,

    #[command(flatten)]
    pub load: Load,

    /// Stop after this many instructions.
    ///
    /// Unlimited by default; a program that loops forever runs until interrupted.
    #[arg(long, value_name = "N")]
    pub max_steps: Option<u64>,

    /// How `Input` reads a typed line. By default one value per line.
    ///
    /// `utf16` reads the line as text instead, spending one UTF-16 code unit per
    /// `Input`, so a single typed string feeds a program that reads one character at a
    /// time. This is the MARIE.js "Inputs" panel's Unicode (UTF-16BE) mode.
    #[arg(long, value_enum, value_name = "MODE", default_value_t = InputMode::Word)]
    pub input: InputMode,

    /// Print the registers when the program stops.
    #[arg(long)]
    pub registers: bool,

    /// Draw the memory-mapped display as the program runs.
    #[arg(short, long)]
    pub display: bool,

    /// Instructions to run between display refreshes.
    #[arg(long, value_name = "N", default_value_t = 3_000, requires = "display")]
    pub refresh: u64,

    /// Pace execution, 0 (slowest) to 9 (unlimited), as the MARIE.js slider does.
    ///
    /// The levels count register transfers rather than whole instructions, so level 0
    /// advances the machine one micro-operation per second.
    #[arg(short, long, value_name = "LEVEL", value_parser = parse_speed)]
    pub speed: Option<Speed>,

    /// List the speed levels and exit.
    #[arg(long)]
    pub speeds: bool,
}

/// Parses a slider position.
fn parse_speed(written: &str) -> Result<Speed, String> {
    let level: u8 = written
        .parse()
        .map_err(|_| format!("'{written}' is not a speed level"))?;
    Speed::new(level).ok_or_else(|| format!("speed must be between 0 and {}", Speed::LEVELS - 1))
}

/// Assembles and runs the file.
pub fn run(args: Args) -> miette::Result<()> {
    if args.speeds {
        for speed in Speed::all() {
            println!("{speed}");
        }
        return Ok(());
    }

    let file = args
        .file
        .clone()
        .expect("clap requires a file unless --speeds was given");
    let program = input::load(&file, &args.load)?;

    let stop = Interrupt::install();

    let mut vm = MarieVM::new(StdinIo::with_mode(args.input.into()));
    program.install(&mut vm)?;

    // A paced run drives micro-operations, which is the unit the slider counts.
    if let Some(speed) = args.speed
        && !speed.is_unlimited()
    {
        return pace(vm.debug(), &args, speed, &stop);
    }
    if args.display {
        return animate(vm.boot(), &args, &stop);
    }

    let (outcome, executed) = uninterrupted(vm.boot(), &args, &stop);
    match outcome {
        RunOutcome::Terminated(vm) => {
            if args.registers {
                println!("{}", vm.registers());
            }
            Ok(())
        }
        RunOutcome::Faulted(vm, fault) => {
            if args.registers {
                println!("{}", vm.registers());
            }
            Err(miette::miette!(
                help = "the machine stopped at the address named above",
                "{fault}"
            ))
        }
        RunOutcome::Suspended(vm, reason) => {
            if stop.requested() {
                interrupt::stopped(vm.registers().pc, executed);
            }
            Err(miette::miette!(
                help = "raise --max-steps if the program is meant to run this long",
                "{reason}"
            ))
        }
    }
}

/// Runs the program to completion, in slices so Ctrl-C is noticed promptly.
///
/// `run` and `run_bounded` are tight loops inside the VM with no way to look up, so
/// interruptibility has to come from returning to this loop often enough. A million
/// instructions is far below a perceptible pause and costs nothing measurable.
fn uninterrupted(
    mut running: MarieVM<StdinIo, mrs_vm::states::Running>,
    args: &Args,
    stop: &Interrupt,
) -> (RunOutcome<StdinIo>, u64) {
    const SLICE: u64 = 1_000_000;
    let mut executed = 0u64;
    loop {
        if stop.requested() {
            return (
                RunOutcome::Suspended(running, mrs_vm::states::SuspendReason::StepLimit),
                executed,
            );
        }
        let slice = match args.max_steps {
            Some(limit) => SLICE.min(limit.saturating_sub(executed)),
            None => SLICE,
        };
        if slice == 0 {
            return (
                RunOutcome::Suspended(running, mrs_vm::states::SuspendReason::StepLimit),
                executed,
            );
        }
        match running.run_bounded(slice) {
            RunOutcome::Suspended(vm, mrs_vm::states::SuspendReason::StepLimit) => {
                running = vm;
                executed += slice;
            }
            other => return (other, executed),
        }
    }
}

/// Runs the program in slices, redrawing the display between them.
///
/// The VM has no notion of time, so "animation" is just choosing how often to look.
/// Only a changed picture is redrawn, so a program that computes for a long while
/// without drawing does not flicker.
fn animate(
    mut running: MarieVM<StdinIo, mrs_vm::states::Running>,
    args: &Args,
    stop: &Interrupt,
) -> miette::Result<()> {
    let mut executed = 0u64;
    let mut first = true;
    let mut previous = running.display().to_words();
    display::draw(&running.display(), &mut first);

    loop {
        if stop.requested() {
            display::draw(&running.display(), &mut first);
            interrupt::stopped(running.registers().pc, executed);
        }
        // Honour the overall budget while still stopping often enough to redraw.
        let slice = match args.max_steps {
            Some(limit) => args.refresh.min(limit.saturating_sub(executed)),
            None => args.refresh,
        };
        if slice == 0 {
            return Err(miette::miette!(
                help = "raise --max-steps if the program is meant to run this long",
                "step limit reached"
            ));
        }

        match running.run_bounded(slice) {
            RunOutcome::Suspended(vm, reason) => {
                running = vm;
                executed += slice;
                let current = running.display().to_words();
                if current != previous {
                    display::draw(&running.display(), &mut first);
                    previous = current;
                }
                // Only the step limit is expected here; a breakpoint or a stall is not.
                if !matches!(reason, mrs_vm::states::SuspendReason::StepLimit) {
                    return Err(miette::miette!("{reason}"));
                }
            }
            RunOutcome::Terminated(vm) => {
                display::draw(&vm.display(), &mut first);
                if args.registers {
                    println!("{}", vm.registers());
                }
                return Ok(());
            }
            RunOutcome::Faulted(vm, fault) => {
                display::draw(&vm.display(), &mut first);
                return Err(miette::miette!("{fault}"));
            }
        }
    }
}

/// Runs the program at a slider speed, redrawing the display between batches.
///
/// The batch is cut short at [`Speed::BATCH_TIME_LIMIT`] however many steps are left to
/// run, which is what keeps a fast level from freezing the terminal between repaints.
fn pace(
    mut stepping: MarieVM<StdinIo, mrs_vm::states::Stepping>,
    args: &Args,
    speed: Speed,
    stop: &Interrupt,
) -> miette::Result<()> {
    let mut first = true;
    let mut previous = stepping.display().to_words();
    if args.display {
        display::draw(&stepping.display(), &mut first);
    }
    let mut instructions = 0u64;

    loop {
        if stop.requested() {
            if args.display {
                display::draw(&stepping.display(), &mut first);
            }
            interrupt::stopped(stepping.registers().pc, instructions);
        }
        let started = Instant::now();
        let budget = speed.micro_steps().unwrap_or(u64::MAX);
        for _ in 0..budget {
            if started.elapsed() >= Speed::BATCH_TIME_LIMIT || stop.requested() {
                break;
            }
            if args.max_steps.is_some_and(|limit| instructions >= limit) {
                return Err(miette::miette!(
                    help = "raise --max-steps if the program is meant to run this long",
                    "step limit reached"
                ));
            }
            match stepping.micro_step() {
                MicroStepOutcome::Stepped(vm, _) => {
                    stepping = vm;
                    // A completed cycle returns the machine to a boundary.
                    if stepping.at_instruction_boundary() {
                        instructions += 1;
                    }
                }
                MicroStepOutcome::AwaitingInput(vm) => stepping = vm,
                MicroStepOutcome::Terminated(vm) => {
                    if args.display {
                        display::draw(&vm.display(), &mut first);
                    }
                    if args.registers {
                        println!("{}", vm.registers());
                    }
                    return Ok(());
                }
                MicroStepOutcome::Faulted(vm, fault) => {
                    if args.display {
                        display::draw(&vm.display(), &mut first);
                    }
                    return Err(miette::miette!("{fault}"));
                }
            }
        }

        if args.display {
            let current = stepping.display().to_words();
            if current != previous {
                display::draw(&stepping.display(), &mut first);
                previous = current;
            }
        }
        // The library never sleeps; a terminal driver blocks, a browser would not.
        // Sleeping in slices is what keeps Ctrl-C responsive at the slow levels, where
        // a single delay is a whole second.
        if speed.is_delayed() {
            stop.sleep(speed.delay());
        }
    }
}
