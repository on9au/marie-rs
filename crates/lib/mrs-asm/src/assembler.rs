//! The second pass: a [`Program`] to machine words.
//!
//! Each item becomes exactly one word, so an item that fails to assemble still emits a
//! zero — as MARIE.js does — and the word list stays aligned with the item list. That
//! keeps the source map meaningful even for a file full of errors, which is the state
//! an editor sees most of the time.
//!
//! # Operand resolution
//!
//! The rule worth spelling out is how an operand is told from a label. MARIE.js tests
//! it with `/^\d[0-9a-fA-F]*$/`: **an operand that starts with a decimal digit is a
//! hexadecimal address**, and anything else is a label reference. So `Load 1A` reads
//! address `0x1A`, while `Load A1` refers to a label called `A1`. Operands are always
//! hexadecimal regardless of which directive is nearby; only `DEC`, `OCT` and `HEX`
//! literals use another base, and those never go through this path.

use mrs_core::{Directive, Instruction, MemoryAddress, Opcode, literal};

use crate::ast::{Item, MnemonicKind, Operand, Program};
use crate::diagnostic::{Code, Diagnostic, Sink};
use crate::symbols::SymbolTable;

/// Assembles `program`, appending diagnostics and recording label references.
///
/// Returns one word per item, in order.
pub fn assemble_program(
    program: &Program,
    symbols: &mut SymbolTable,
    diagnostics: &mut dyn Sink,
) -> Vec<i16> {
    program
        .items
        .iter()
        .map(|item| assemble_item(item, symbols, diagnostics))
        .collect()
}

/// Assembles one item into a word, emitting zero if it cannot be assembled.
fn assemble_item(item: &Item, symbols: &mut SymbolTable, diagnostics: &mut dyn Sink) -> i16 {
    // `DEC`, `OCT` and `HEX` emit a bare literal rather than an instruction.
    if let MnemonicKind::Directive(directive) = item.mnemonic.kind
        && let Some(radix) = directive.literal_radix()
    {
        return assemble_literal(item, radix, diagnostics);
    }

    // `ADR x` is `JnS x` and `Clear` is `LoadImmi 0`; both then take the ordinary
    // instruction path.
    let (opcode, operand) = match resolve_alias(item, diagnostics) {
        Some(resolved) => resolved,
        None => return 0,
    };

    let needs_operand = opcode.takes_operand();
    let Some(operand) = operand else {
        if needs_operand {
            diagnostics.push(Diagnostic::error(
                Code::MISSING_OPERAND,
                item.span,
                item.line,
                format!("Expected operand for {}.", item.mnemonic.lowercase),
            ));
            return 0;
        }
        return Instruction::new(opcode, MemoryAddress::ZERO)
            .encode()
            .value();
    };

    if !needs_operand {
        diagnostics.push(Diagnostic::error(
            Code::UNEXPECTED_OPERAND,
            operand.span,
            item.line,
            format!(
                "Unexpected operand '{}' for '{}'.",
                operand.text, item.mnemonic.lowercase
            ),
        ));
        return 0;
    }

    let Some(address) = resolve_operand(item, &operand, symbols, diagnostics) else {
        return 0;
    };

    Instruction::new(opcode, address).encode().value()
}

/// Emits a `DEC`, `OCT` or `HEX` literal.
fn assemble_literal(item: &Item, radix: literal::Radix, diagnostics: &mut dyn Sink) -> i16 {
    let Some(operand) = &item.operand else {
        diagnostics.push(Diagnostic::error(
            Code::MISSING_OPERAND,
            item.span,
            item.line,
            "Expected operand.",
        ));
        return 0;
    };

    match literal::parse_word(&operand.text, radix) {
        Ok(value) => value.value(),
        // MARIE.js validates the digits and the range separately; `parse_word` folds
        // both into one call, so the error variant decides which message to give.
        Err(literal::ParseWordError::OutOfRange) => {
            diagnostics.push(Diagnostic::error(
                Code::LITERAL_OUT_OF_RANGE,
                operand.span,
                item.line,
                "Literal out of bounds.",
            ));
            0
        }
        Err(_) => {
            diagnostics.push(Diagnostic::error(
                Code::MALFORMED_LITERAL,
                operand.span,
                item.line,
                "Failed to parse operand.",
            ));
            0
        }
    }
}

/// Resolves the mnemonic to an opcode, rewriting the `ADR` and `Clear` aliases.
///
/// Returns `None` after reporting an unknown mnemonic.
fn resolve_alias(item: &Item, diagnostics: &mut dyn Sink) -> Option<(Opcode, Option<Operand>)> {
    match item.mnemonic.kind {
        MnemonicKind::Opcode(opcode) => Some((opcode, item.operand.clone())),
        MnemonicKind::Directive(Directive::Adr) => {
            // `ADR` keeps its operand and becomes `JnS`, whose opcode is zero, so the
            // word ends up being the bare address.
            Some((Opcode::JnS, item.operand.clone()))
        }
        MnemonicKind::Directive(Directive::Clear) => {
            // `Clear` becomes `LoadImmi 0`. MARIE.js overwrites any operand that was
            // written, so `Clear 5` is silently `LoadImmi 0` rather than an error; the
            // `ignored-operand` lint flags it without changing the output.
            Some((
                Opcode::LoadImmi,
                Some(Operand {
                    text: "0".to_owned(),
                    span: item.mnemonic.span,
                }),
            ))
        }
        // `ORG` and `END` steer the parser and never reach here in their valid forms.
        // An `ORG` that failed its strict three-hex-digit shape falls through to this
        // pass, where MARIE.js reports it as an unknown operator; match that.
        MnemonicKind::Directive(Directive::Org | Directive::End) | MnemonicKind::Unknown => {
            diagnostics.push(Diagnostic::error(
                Code::UNKNOWN_MNEMONIC,
                item.mnemonic.span,
                item.line,
                format!("Unknown operator '{}'.", item.mnemonic.lowercase),
            ));
            None
        }
        MnemonicKind::Directive(Directive::Dec | Directive::Oct | Directive::Hex) => {
            // Handled by `assemble_literal` before this function is reached.
            unreachable!("literal directives take the literal path")
        }
    }
}

/// Turns an operand into an address, either by parsing it or by looking it up.
fn resolve_operand(
    item: &Item,
    operand: &Operand,
    symbols: &mut SymbolTable,
    diagnostics: &mut dyn Sink,
) -> Option<MemoryAddress> {
    if operand.looks_like_address_literal() {
        return parse_address_literal(item, operand, diagnostics);
    }
    match symbols.reference(&operand.text, operand.span) {
        Some(address) => Some(address),
        None => {
            diagnostics.push(
                Diagnostic::error(
                    Code::UNKNOWN_LABEL,
                    operand.span,
                    item.line,
                    format!("Unknown label '{}'.", operand.text),
                )
                .with_help(unknown_label_help(&operand.text)),
            );
            None
        }
    }
}

/// Suggests a fix for an operand that was read as a label but probably was not meant
/// to be one.
///
/// An operand only counts as a literal if it starts with a *decimal* digit, so an
/// address written `C00` or `FFF` silently becomes a label reference. This is the trap
/// behind `Skipcond C00`, which is why MARIE.js's own documentation spells that one
/// condition `0C00` while the other three are plain `000`, `400` and `800`. When the
/// operand looks like it was meant to be an address, say so instead of giving the
/// generic advice.
fn unknown_label_help(text: &str) -> String {
    let looks_like_hex = !text.is_empty() && text.chars().all(|c| c.is_ascii_hexdigit());
    let fits_the_address_field = text.trim_start_matches('0').len() <= 3;
    if looks_like_hex && fits_the_address_field {
        return format!(
            "If `{text}` was meant to be an address, write it as `0{text}`: an operand \
             is only read as a hexadecimal literal when it starts with a decimal digit."
        );
    }
    "Define it as `label, ...`, or write a hexadecimal address starting with a digit.".to_owned()
}

/// Parses a hexadecimal address literal, which must fit in the 12-bit address field.
fn parse_address_literal(
    item: &Item,
    operand: &Operand,
    diagnostics: &mut dyn Sink,
) -> Option<MemoryAddress> {
    // Leading zeros are allowed and carry no weight, so `0FFF` is in range while `1000`
    // is not. Stripping them first also keeps an absurdly long literal from overflowing
    // the integer parse.
    let digits = operand.text.trim_start_matches('0');
    let value = if digits.is_empty() {
        Some(0)
    } else if digits.len() <= 3 {
        u16::from_str_radix(digits, 16).ok()
    } else {
        None
    };

    match value.and_then(MemoryAddress::try_new) {
        Some(address) => Some(address),
        None => {
            diagnostics.push(
                Diagnostic::error(
                    Code::ADDRESS_OUT_OF_RANGE,
                    operand.span,
                    item.line,
                    format!("Address 0x{} is out of bounds.", operand.text),
                )
                .with_help("The address field is 12 bits, so the operand must be at most 0xFFF."),
            );
            None
        }
    }
}
