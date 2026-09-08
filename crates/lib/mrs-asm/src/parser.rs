//! The first pass: source text to a [`Program`], with labels resolved to addresses.
//!
//! This mirrors MARIE.js's first pass exactly, including the parts that are surprising:
//!
//! - a line whose label is invalid is **skipped entirely**, so it produces no word and
//!   the addresses of everything after it shift down,
//! - `ORG` is only recognised in its strict three-hex-digit form, and an `ORG` that
//!   arrives too late is reported but ignored, leaving the origin as it was,
//! - a label on an `END` line is still defined, at the address one past the last word,
//! - nothing after `END` is looked at, so no diagnostics are reported there.
//!
//! Addresses are assigned here rather than in the assembler because MARIE.js assigns
//! them as it parses: a label's address is the number of words emitted so far plus the
//! origin, which is only knowable in source order.

use mrs_core::{Directive, MemoryAddress, Opcode};

use crate::ast::{End, Item, Label, Mnemonic, MnemonicKind, Operand, Origin, Program};
use crate::diagnostic::{Code, Diagnostic, Sink};
use crate::lexer::{LineForm, split_line};
use crate::source_map::SourceMap;
use crate::span::Span;
use crate::symbols::SymbolTable;

/// What the first pass produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed {
    /// The syntax tree.
    pub program: Program,
    /// Every label, with its definition site and address.
    pub symbols: SymbolTable,
    /// Address to source line, as MARIE.js builds it.
    pub source_map: SourceMap,
}

/// Parses `source` into a [`Program`], reporting every problem it finds to `sink`.
pub fn parse(source: &str, sink: &mut dyn Sink) -> Parsed {
    let mut out = Parsed::default();
    let diagnostics = sink;
    let mut base = 0u32;
    let mut stopped = false;

    for (line_number, line) in source.split('\n').enumerate() {
        let line_number = line_number as u32;
        if stopped {
            // Everything after `END` is invisible to MARIE.js. Record where the dead
            // region starts so the lint pass can mention it, but report nothing here.
            if out.program.trailing.is_none() && !line.trim().is_empty() {
                let span = trimmed_span(line, base);
                out.program.trailing = Some(Span::new(span.start, source.len() as u32));
            }
            base += line.len() as u32 + 1;
            continue;
        }

        match split_line(line, base) {
            LineForm::Blank { .. } => {}
            LineForm::Origin {
                keyword,
                digits,
                comment: _,
            } => parse_origin(&mut out, diagnostics, keyword, digits, line_number, source),
            LineForm::Statement {
                label,
                mnemonic,
                operand,
                comment,
            } => {
                stopped = parse_statement(
                    &mut out,
                    diagnostics,
                    source,
                    line_number,
                    label,
                    mnemonic,
                    operand,
                    comment,
                );
            }
            LineForm::Malformed => {
                let span = trimmed_span(line, base);
                diagnostics.push(
                    Diagnostic::error(
                        Code::MALFORMED_LINE,
                        span,
                        line_number,
                        "Line has incorrect form.",
                    )
                    .with_help("Expected `label, mnemonic operand / comment`."),
                );
            }
        }

        base += line.len() as u32 + 1;
    }

    out
}

/// Handles a well-formed `ORG hhh` line.
fn parse_origin(
    out: &mut Parsed,
    diagnostics: &mut dyn Sink,
    keyword: Span,
    digits: Span,
    line: u32,
    source: &str,
) {
    let span = keyword.join(digits);
    // An origin is only allowed before any word has been emitted, and only once.
    if !out.program.items.is_empty() || out.program.origin.is_some() {
        diagnostics.push(Diagnostic::error(
            Code::UNEXPECTED_ORIGIN,
            span,
            line,
            "Unexpected origination directive.",
        ));
        return;
    }
    let text = digits.text(source).unwrap_or("0");
    // The lexer already checked that these are exactly three hex digits, so the parse
    // cannot fail and the value cannot exceed 0xFFF.
    let address = u16::from_str_radix(text, 16).unwrap_or(0);
    out.program.origin = Some(Origin {
        address: MemoryAddress::new_masked(address),
        digits,
        span,
        line,
    });
}

/// Handles a statement line. Returns `true` if it was `END` and parsing should stop.
#[allow(clippy::too_many_arguments)]
fn parse_statement(
    out: &mut Parsed,
    diagnostics: &mut dyn Sink,
    source: &str,
    line: u32,
    label: Option<Span>,
    mnemonic_span: Span,
    operand_span: Option<Span>,
    comment: Option<Span>,
) -> bool {
    let text = mnemonic_span.text(source).unwrap_or_default();
    let lowercase = text.to_ascii_lowercase();

    // The address this line will occupy: the number of words already emitted, plus the
    // origin. Labels are recorded against it before the `END` check, so a label on an
    // `END` line points one past the program.
    let Some(address) = next_address(out) else {
        // Past the end of memory. Report once, on the first line that overflows, and
        // stop assigning addresses rather than panicking or silently wrapping.
        diagnostics.push(Diagnostic::error(
            Code::PROGRAM_TOO_LARGE,
            mnemonic_span,
            line,
            format!(
                "Program does not fit in the {}-word address space.",
                mrs_core::MEMORY_WORD_COUNT
            ),
        ));
        return true;
    };

    if let Some(label_span) = label
        && !define_label(out, diagnostics, source, label_span, address, line)
    {
        // An invalid label skips the whole line, so no word is emitted and the next
        // statement takes this address instead.
        return false;
    }

    out.source_map.insert(address, line);

    if lowercase == "end" {
        out.program.end = Some(End {
            span: mnemonic_span,
            line,
        });
        return true;
    }

    let kind = classify(&lowercase);
    let span = match operand_span {
        Some(operand) => mnemonic_span.join(operand),
        None => mnemonic_span,
    };

    out.program.items.push(Item {
        label: label.map(|span| Label {
            name: span.text(source).unwrap_or_default().to_owned(),
            span,
        }),
        mnemonic: Mnemonic {
            text: text.to_owned(),
            lowercase,
            kind,
            span: mnemonic_span,
        },
        operand: operand_span.map(|span| Operand {
            text: span.text(source).unwrap_or_default().to_owned(),
            span,
        }),
        comment,
        address,
        line,
        span,
    });
    false
}

/// The address the next word will occupy, or `None` if it is past the end of memory.
fn next_address(out: &Parsed) -> Option<MemoryAddress> {
    let origin = out.program.origin_address().value() as usize;
    let index = origin.checked_add(out.program.items.len())?;
    MemoryAddress::try_new(u16::try_from(index).ok()?)
}

/// Records a label definition. Returns `false` if the label was rejected, in which case
/// the caller must skip the line.
fn define_label(
    out: &mut Parsed,
    diagnostics: &mut dyn Sink,
    source: &str,
    span: Span,
    address: MemoryAddress,
    line: u32,
) -> bool {
    let name = span.text(source).unwrap_or_default();

    if name.starts_with(|c: char| c.is_ascii_digit()) {
        diagnostics.push(Diagnostic::error(
            Code::LABEL_STARTS_WITH_DIGIT,
            span,
            line,
            "Labels cannot start with a number.",
        ));
        return false;
    }
    if name.contains(char::is_whitespace) {
        diagnostics.push(Diagnostic::error(
            Code::LABEL_CONTAINS_WHITESPACE,
            span,
            line,
            "Labels cannot contain whitespace.",
        ));
        return false;
    }
    if let Err(existing) = out.symbols.define(name, address, span, line) {
        let (first_span, first_line) = (existing.definition, existing.line);
        diagnostics.push(
            Diagnostic::error(
                Code::DUPLICATE_LABEL,
                span,
                line,
                format!(
                    "Labels must be unique. The label '{name}' was already defined on line {}.",
                    first_line + 1
                ),
            )
            .with_label(first_span, "first defined here"),
        );
        return false;
    }
    true
}

/// Resolves a lowercased mnemonic to an opcode or a directive.
fn classify(lowercase: &str) -> MnemonicKind {
    if let Some(opcode) = Opcode::from_mnemonic(lowercase) {
        return MnemonicKind::Opcode(opcode);
    }
    if let Some(directive) = Directive::from_mnemonic(lowercase) {
        return MnemonicKind::Directive(directive);
    }
    MnemonicKind::Unknown
}

/// The span of a line with its surrounding whitespace removed.
fn trimmed_span(line: &str, base: u32) -> Span {
    let end = line.trim_end();
    let start = end.len() - end.trim_start().len();
    Span::new(base + start as u32, base + end.len() as u32)
}
