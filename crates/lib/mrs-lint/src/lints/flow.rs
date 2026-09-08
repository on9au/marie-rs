//! Lints about where control can go.

use mrs_asm::diagnostic::{Code, Diagnostic, Sink};
use mrs_asm::lint::{Lint, LintContext};

use crate::LINT;
use crate::cfg::{Edge, Kind};

/// Execution can run past the last assembled word.
///
/// MARIE keeps fetching from the next address whatever is there, so a program that
/// falls off the end executes whatever memory happens to hold — usually zeros, which
/// decode as `JnS 000` and scribble over address zero.
#[derive(Debug, Clone, Copy, Default)]
pub struct FallsOffEnd;

/// The code reported by [`FallsOffEnd`].
pub const FALLS_OFF_END: Code = Code::new(LINT, "falls-off-end");

impl Lint for FallsOffEnd {
    fn code(&self) -> Code {
        FALLS_OFF_END
    }

    fn description(&self) -> &'static str {
        "execution can continue past the last word of the program"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let Some(cfg) = crate::graph(cx) else { return };
        for index in cfg.reachable_indices() {
            if cfg.kind(index) != Some(Kind::Code) {
                continue;
            }
            if !cfg.edges(cx.assembly, index).contains(&Edge::OffEnd) {
                continue;
            }
            let item = &cx.assembly.program.items[index];
            out.push(
                Diagnostic::warning(
                    self.code(),
                    item.span,
                    item.line,
                    "Execution can continue past the end of the program from here.",
                )
                .with_help("End the path with `Halt`, or jump somewhere that does."),
            );
        }
    }
}

/// Execution can reach a word that was written as data.
///
/// A `DEC 21` reached by the program counter is decoded like any other word: `21` is
/// `0x0015`, which is `JnS 015`. This is the usual shape of a missing `Halt` between
/// the last instruction and the variables underneath it.
#[derive(Debug, Clone, Copy, Default)]
pub struct FallsIntoData;

/// The code reported by [`FallsIntoData`].
pub const FALLS_INTO_DATA: Code = Code::new(LINT, "falls-into-data");

impl Lint for FallsIntoData {
    fn code(&self) -> Code {
        FALLS_INTO_DATA
    }

    fn description(&self) -> &'static str {
        "execution can reach a word that was declared as data"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let Some(cfg) = crate::graph(cx) else { return };
        for index in cfg.reachable_indices() {
            if cfg.kind(index) != Some(Kind::Data) {
                continue;
            }
            let item = &cx.assembly.program.items[index];
            // Naming the instruction the data decodes as is what makes this concrete.
            let decoded =
                mrs_core::Instruction::decode(mrs_core::Value::new(cx.assembly.words[index]));
            let effect = match decoded {
                Some(instruction) => format!("executed as `{instruction}`"),
                None => "executed as an invalid instruction".to_owned(),
            };
            out.push(
                Diagnostic::warning(
                    self.code(),
                    item.span,
                    item.line,
                    format!("This data word can be reached by execution, and would be {effect}."),
                )
                .with_help(
                    "Put a `Halt` or a `Jump` above the data so control cannot fall into it.",
                ),
            );
        }
    }
}

/// A word that nothing can reach.
///
/// Only reported when the program has no indirect control transfers, because a
/// `JumpI` can land anywhere and would make this a guess.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnreachableInstruction;

/// The code reported by [`UnreachableInstruction`].
pub const UNREACHABLE_INSTRUCTION: Code = Code::new(LINT, "unreachable-instruction");

impl Lint for UnreachableInstruction {
    fn code(&self) -> Code {
        UNREACHABLE_INSTRUCTION
    }

    fn description(&self) -> &'static str {
        "a word cannot be reached by any path from the entry point"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let Some(cfg) = crate::graph(cx) else { return };
        // An indirect transfer hides edges, so absence of a path proves nothing.
        if !cfg.exact() {
            return;
        }
        for (index, item) in cx.assembly.program.items.iter().enumerate() {
            // Data that nothing reaches is normal — that is what a variable is. Only
            // unreachable *code* is worth a word.
            if cfg.is_reachable(index) || cfg.kind(index) != Some(Kind::Code) {
                continue;
            }
            out.push(
                Diagnostic::warning(
                    self.code(),
                    item.span,
                    item.line,
                    "This instruction can never be reached.",
                )
                .with_help("Remove it, or add a jump that reaches it."),
            );
        }
    }
}

/// Control leaves the assembled program.
#[derive(Debug, Clone, Copy, Default)]
pub struct JumpsOutsideProgram;

/// The code reported by [`JumpsOutsideProgram`].
pub const JUMPS_OUTSIDE: Code = Code::new(LINT, "jumps-outside-program");

impl Lint for JumpsOutsideProgram {
    fn code(&self) -> Code {
        JUMPS_OUTSIDE
    }

    fn description(&self) -> &'static str {
        "a jump targets an address the program does not occupy"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let Some(cfg) = crate::graph(cx) else { return };
        for index in cfg.reachable_indices() {
            let item = &cx.assembly.program.items[index];
            for edge in cfg.edges(cx.assembly, index) {
                let Edge::Outside(address) = edge else {
                    continue;
                };
                out.push(
                    Diagnostic::warning(
                        self.code(),
                        item.span,
                        item.line,
                        format!(
                            "This transfers control to {address}, which is outside the program."
                        ),
                    )
                    .with_help(
                        "That memory is zero-filled, so execution would run into `JnS 000`.",
                    ),
                );
            }
        }
    }
}
