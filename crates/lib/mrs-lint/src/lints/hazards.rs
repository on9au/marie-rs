//! Lints for constructs that assemble cleanly and then do the wrong thing.

use mrs_asm::diagnostic::{Code, Diagnostic, Sink};
use mrs_asm::lint::{Lint, LintContext};
use mrs_core::{Opcode, SkipCondition};

use crate::LINT;
use crate::cfg::{Kind, effective_opcode};

/// A `Skipcond` whose operand is not one of the four conditions.
///
/// Only bits 11-10 of the operand select the condition; MARIE.js masks with `0xC00`
/// and ignores the rest. So `Skipcond 100` is not a fifth condition and not an error —
/// it silently behaves as `Skipcond 000`. This is the one MARIE mistake that produces
/// a working program that tests the wrong thing.
#[derive(Debug, Clone, Copy, Default)]
pub struct MaskedSkipcond;

/// The code reported by [`MaskedSkipcond`].
pub const MASKED_SKIPCOND: Code = Code::new(LINT, "masked-skipcond");

impl Lint for MaskedSkipcond {
    fn code(&self) -> Code {
        MASKED_SKIPCOND
    }

    fn description(&self) -> &'static str {
        "a Skipcond operand has bits that are silently ignored"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        for (index, item) in cx.assembly.program.items.iter().enumerate() {
            if effective_opcode(item) != Some(Opcode::SkipCond) {
                continue;
            }
            let Some(word) = cx.assembly.words.get(index) else {
                continue;
            };
            let operand = *word as u16 & 0x0FFF;
            // The low ten bits select nothing, so anything set there is a typo.
            if operand & 0x03FF == 0 {
                continue;
            }
            let condition = SkipCondition::from_operand(mrs_core::MemoryAddress::new(operand));
            let canonical = condition.to_operand();
            let Some(operand_node) = &item.operand else {
                continue;
            };
            out.push(
                Diagnostic::warning(
                    self.code(),
                    operand_node.span,
                    item.line,
                    format!(
                        "Skipcond {operand:03X} is not a condition: only bits 11-10 of the \
                         operand are read, so this silently behaves as `Skipcond {canonical}` \
                         and tests `{condition}`."
                    ),
                )
                .with_help(
                    "The four conditions are 000 (AC < 0), 400 (AC = 0), 800 (AC > 0) and \
                     0C00 (AC != 0). Note that 0C00 needs the leading zero.",
                ),
            );
        }
    }
}

/// A `Skipcond` whose operand is a label.
///
/// The operand of a `Skipcond` is a condition selector, not an address, so resolving it
/// through the symbol table means the label's address is reinterpreted as a condition —
/// whichever one bits 11-10 of that address happen to name.
#[derive(Debug, Clone, Copy, Default)]
pub struct SkipcondLabelOperand;

/// The code reported by [`SkipcondLabelOperand`].
pub const SKIPCOND_LABEL_OPERAND: Code = Code::new(LINT, "skipcond-label-operand");

impl Lint for SkipcondLabelOperand {
    fn code(&self) -> Code {
        SKIPCOND_LABEL_OPERAND
    }

    fn description(&self) -> &'static str {
        "a Skipcond operand is a label rather than a condition"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        for item in &cx.assembly.program.items {
            if effective_opcode(item) != Some(Opcode::SkipCond) {
                continue;
            }
            let Some(operand) = &item.operand else {
                continue;
            };
            if operand.looks_like_address_literal() {
                continue;
            }
            let Some(symbol) = cx.assembly.symbols.get(&operand.text) else {
                continue;
            };
            let condition = SkipCondition::from_operand(symbol.address);
            out.push(
                Diagnostic::warning(
                    self.code(),
                    operand.span,
                    item.line,
                    format!(
                        "'{}' is a label at {}, so this tests `{condition}` rather than \
                         branching to it.",
                        operand.text, symbol.address
                    ),
                )
                .with_label(symbol.definition, "the label is defined here")
                .with_help(
                    "Skipcond takes a condition, not an address. Use `Jump` to branch, or \
                     write one of 000, 400, 800 or 0C00.",
                ),
            );
        }
    }
}

/// A `JnS` whose return slot holds an instruction.
///
/// `JnS X` writes the return address into `M[X]` and then continues at `X + 1`, so `X`
/// must be a spare word. Pointing it at code overwrites that instruction the first time
/// the subroutine is called.
#[derive(Debug, Clone, Copy, Default)]
pub struct JnsOverwritesCode;

/// The code reported by [`JnsOverwritesCode`].
pub const JNS_OVERWRITES_CODE: Code = Code::new(LINT, "jns-overwrites-code");

impl Lint for JnsOverwritesCode {
    fn code(&self) -> Code {
        JNS_OVERWRITES_CODE
    }

    fn description(&self) -> &'static str {
        "a JnS return slot holds an instruction that the call will overwrite"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let Some(cfg) = crate::graph(cx) else { return };
        for (index, item) in cx.assembly.program.items.iter().enumerate() {
            if effective_opcode(item) != Some(Opcode::JnS) {
                continue;
            }
            let Some(word) = cx.assembly.words.get(index) else {
                continue;
            };
            let target = mrs_core::MemoryAddress::new_masked(*word as u16);
            let Some(slot) = cfg.index_of(target) else {
                continue;
            };
            if cfg.kind(slot) != Some(Kind::Code) {
                continue;
            }
            let victim = &cx.assembly.program.items[slot];
            out.push(
                Diagnostic::warning(
                    self.code(),
                    item.span,
                    item.line,
                    format!(
                        "This call writes its return address to {target}, overwriting the \
                         instruction there."
                    ),
                )
                .with_label(victim.span, "this instruction is overwritten")
                .with_help("Reserve a word for the return address, such as `Sub, HEX 0`."),
            );
        }
    }
}

/// A `Store` that targets an instruction.
///
/// Self-modifying code is legal and occasionally deliberate, so this is advice rather
/// than a warning — but it is worth being sure it was meant.
#[derive(Debug, Clone, Copy, Default)]
pub struct SelfModifyingCode;

/// The code reported by [`SelfModifyingCode`].
pub const SELF_MODIFYING_CODE: Code = Code::new(LINT, "self-modifying-code");

impl Lint for SelfModifyingCode {
    fn code(&self) -> Code {
        SELF_MODIFYING_CODE
    }

    fn description(&self) -> &'static str {
        "a Store writes over a word that is executed as an instruction"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let Some(cfg) = crate::graph(cx) else { return };
        for (index, item) in cx.assembly.program.items.iter().enumerate() {
            if effective_opcode(item) != Some(Opcode::Store) {
                continue;
            }
            let Some(word) = cx.assembly.words.get(index) else {
                continue;
            };
            let target = mrs_core::MemoryAddress::new_masked(*word as u16);
            let Some(slot) = cfg.index_of(target) else {
                continue;
            };
            // Only interesting if the overwritten word is code that actually runs.
            if cfg.kind(slot) != Some(Kind::Code) || !cfg.is_reachable(slot) {
                continue;
            }
            let victim = &cx.assembly.program.items[slot];
            out.push(
                Diagnostic::advice(
                    self.code(),
                    item.span,
                    item.line,
                    format!("This writes over the instruction at {target}."),
                )
                .with_label(victim.span, "this instruction is modified at run time")
                .with_help("If that is deliberate, a comment saying so will save the next reader."),
            );
        }
    }
}
