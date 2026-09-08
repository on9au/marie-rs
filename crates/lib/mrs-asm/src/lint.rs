//! Lints: checks that report style problems without changing the output.
//!
//! MARIE.js has no warnings, so nothing here runs unless it is asked for. The point of
//! the [`Lint`] trait is that the set is open: a downstream linter registers its own
//! checks under its own [`Code`] namespace and they flow through the same
//! [`Diagnostic`], the same [`Sink`] and the same renderer as the built-in ones.
//!
//! ```
//! use mrs_asm::diagnostic::{Code, Diagnostic, Sink};
//! use mrs_asm::lint::{Lint, LintContext};
//! use mrs_asm::{Options, assemble_with};
//!
//! /// Flags mnemonics that are not written in the canonical spelling.
//! struct Canonical;
//!
//! const HOUSE_STYLE: Code = Code::new("housestyle", "non-canonical-mnemonic");
//!
//! impl Lint for Canonical {
//!     fn code(&self) -> Code {
//!         HOUSE_STYLE
//!     }
//!
//!     fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
//!         for item in &cx.assembly.program.items {
//!             let Some(opcode) = item.mnemonic.opcode() else { continue };
//!             if item.mnemonic.text != opcode.mnemonic() {
//!                 out.push(
//!                     Diagnostic::warning(
//!                         self.code(),
//!                         item.mnemonic.span,
//!                         item.line,
//!                         format!("Write `{}` rather than `{}`.", opcode.mnemonic(), item.mnemonic.text),
//!                     )
//!                     .with_help("The canonical spelling is easier to grep for."),
//!                 );
//!             }
//!         }
//!     }
//! }
//!
//! let assembly = assemble_with("HALT", Options::with_lints(&[&Canonical]));
//! assert!(assembly.succeeded(), "a lint never blocks assembly");
//! assert_eq!(assembly.warnings().next().unwrap().code, HOUSE_STYLE);
//! ```

use mrs_core::Directive;

use crate::Assembly;
use crate::ast::MnemonicKind;
use crate::diagnostic::{Code, Diagnostic, Sink};

/// What a lint gets to look at.
///
/// Lints run after assembly, so everything is resolved: the syntax tree, the symbol
/// table with its reference sites, the emitted words and the diagnostics already
/// reported.
#[derive(Debug, Clone, Copy)]
pub struct LintContext<'a> {
    /// The finished assembly.
    pub assembly: &'a Assembly,
    /// The source it came from.
    pub source: &'a str,
}

/// A check that runs over a finished [`Assembly`].
///
/// Implementations must not change the emitted words — a lint that would change the
/// program is an error, not a lint.
pub trait Lint {
    /// The code this lint reports under.
    ///
    /// Used to present the lint in a configuration file and to build allow-lists.
    fn code(&self) -> Code;

    /// A one-line description, for `--explain`-style output.
    fn description(&self) -> &'static str {
        ""
    }

    /// Runs the check, reporting to `out`.
    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink);
}

/// Flags labels that nothing refers to.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnusedLabel;

impl Lint for UnusedLabel {
    fn code(&self) -> Code {
        Code::UNUSED_LABEL
    }

    fn description(&self) -> &'static str {
        "a label is defined but never referenced"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        for symbol in cx.assembly.symbols.unused() {
            out.push(
                Diagnostic::warning(
                    self.code(),
                    symbol.definition,
                    symbol.line,
                    format!("Label '{}' is never used.", symbol.name),
                )
                .with_help("Remove it, or reference it from an operand."),
            );
        }
    }
}

/// Flags text after an `END` directive.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnreachableCode;

impl Lint for UnreachableCode {
    fn code(&self) -> Code {
        Code::UNREACHABLE_CODE
    }

    fn description(&self) -> &'static str {
        "code follows an END directive and is never assembled"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let (Some(end), Some(trailing)) = (cx.assembly.program.end, cx.assembly.program.trailing)
        else {
            return;
        };
        out.push(
            Diagnostic::warning(
                self.code(),
                trailing,
                cx.assembly.lines.position(trailing.start).line,
                "Code after END is never assembled.",
            )
            .with_label(end.span, "assembly stops here")
            .with_help("Move this above the END directive, or delete it."),
        );
    }
}

/// Flags an operand the assembler silently discards.
///
/// `Clear` always assembles as `LoadImmi 0`; MARIE.js overwrites whatever operand was
/// written rather than rejecting it, so `Clear 5` quietly loads zero.
#[derive(Debug, Clone, Copy, Default)]
pub struct IgnoredOperand;

impl Lint for IgnoredOperand {
    fn code(&self) -> Code {
        Code::IGNORED_OPERAND
    }

    fn description(&self) -> &'static str {
        "an operand is silently discarded by the assembler"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        for item in &cx.assembly.program.items {
            if item.mnemonic.kind != MnemonicKind::Directive(Directive::Clear) {
                continue;
            }
            let Some(operand) = &item.operand else {
                continue;
            };
            out.push(
                Diagnostic::warning(
                    self.code(),
                    operand.span,
                    item.line,
                    format!(
                        "'{}' is ignored: Clear always assembles as LoadImmi 0.",
                        operand.text
                    ),
                )
                .with_help("Write `LoadImmi <value>` to load something other than zero."),
            );
        }
    }
}

/// The lints this crate ships, in reporting order.
pub const STANDARD: &[&dyn Lint] = &[&UnusedLabel, &UnreachableCode, &IgnoredOperand];

/// Runs `lints` over a finished assembly.
pub fn run(lints: &[&dyn Lint], assembly: &Assembly, source: &str, out: &mut dyn Sink) {
    let cx = LintContext { assembly, source };
    for lint in lints {
        lint.run(&cx, out);
    }
}
