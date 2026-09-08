//! Lints about how the source reads rather than what it does.

use mrs_asm::diagnostic::{Code, Diagnostic, Sink};
use mrs_asm::lint::{Lint, LintContext};
use mrs_core::{Directive, Opcode};

use crate::LINT;

/// A mnemonic not written in its canonical spelling.
#[derive(Debug, Clone, Copy, Default)]
pub struct NonCanonicalMnemonic;

/// The code reported by [`NonCanonicalMnemonic`].
pub const NON_CANONICAL_MNEMONIC: Code = Code::new(LINT, "non-canonical-mnemonic");

impl Lint for NonCanonicalMnemonic {
    fn code(&self) -> Code {
        NON_CANONICAL_MNEMONIC
    }

    fn description(&self) -> &'static str {
        "a mnemonic is not written in its canonical spelling"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        for item in &cx.assembly.program.items {
            let canonical = match item.mnemonic.kind {
                mrs_asm::ast::MnemonicKind::Opcode(opcode) => opcode.mnemonic(),
                mrs_asm::ast::MnemonicKind::Directive(directive) => directive.mnemonic(),
                mrs_asm::ast::MnemonicKind::Unknown => continue,
            };
            if item.mnemonic.text == canonical {
                continue;
            }
            out.push(
                Diagnostic::advice(
                    self.code(),
                    item.mnemonic.span,
                    item.line,
                    format!("Write `{canonical}` rather than `{}`.", item.mnemonic.text),
                )
                .with_help("Consistent spelling makes a program easier to search."),
            );
        }
    }
}

/// A label spelled like an instruction or directive.
///
/// Legal — labels and mnemonics are separate namespaces — but `Load, Load Load` is a
/// sentence nobody should have to parse.
#[derive(Debug, Clone, Copy, Default)]
pub struct LabelShadowsMnemonic;

/// The code reported by [`LabelShadowsMnemonic`].
pub const LABEL_SHADOWS_MNEMONIC: Code = Code::new(LINT, "label-shadows-mnemonic");

impl Lint for LabelShadowsMnemonic {
    fn code(&self) -> Code {
        LABEL_SHADOWS_MNEMONIC
    }

    fn description(&self) -> &'static str {
        "a label is spelled like an instruction or directive"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        for symbol in cx.assembly.symbols.iter() {
            let lowered = symbol.name.to_ascii_lowercase();
            let clashes = Opcode::from_mnemonic(&lowered).is_some()
                || Directive::from_mnemonic(&lowered).is_some();
            if !clashes {
                continue;
            }
            out.push(
                Diagnostic::advice(
                    self.code(),
                    symbol.definition,
                    symbol.line,
                    format!("Label '{}' is spelled like a mnemonic.", symbol.name),
                )
                .with_help("Rename it; mnemonics are matched case-insensitively, so this reads ambiguously."),
            );
        }
    }
}

/// A program with no `Halt` anywhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct MissingHalt;

/// The code reported by [`MissingHalt`].
pub const MISSING_HALT: Code = Code::new(LINT, "missing-halt");

impl Lint for MissingHalt {
    fn code(&self) -> Code {
        MISSING_HALT
    }

    fn description(&self) -> &'static str {
        "the program contains no Halt instruction"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let items = &cx.assembly.program.items;
        if items.is_empty() {
            return;
        }
        if items
            .iter()
            .any(|item| item.mnemonic.opcode() == Some(Opcode::Halt))
        {
            return;
        }
        // Point at the first word, which is where execution starts.
        out.push(
            Diagnostic::warning(
                self.code(),
                items[0].span,
                items[0].line,
                "This program contains no Halt instruction.",
            )
            .with_help("Every path should end at a `Halt`, or the machine runs on into memory."),
        );
    }
}
