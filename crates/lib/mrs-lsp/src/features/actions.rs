//! Quick fixes.
//!
//! Each fix is derived from the syntax tree rather than by parsing a diagnostic's
//! message, so the edits stay correct even when the wording changes. Only mechanical,
//! obviously-safe rewrites are offered — nothing that guesses at intent.

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, Range, TextEdit, Uri,
    WorkspaceEdit,
};
use mrs_asm::ast::MnemonicKind;
use mrs_asm::diagnostic::Code;
use mrs_asm::span::Span;
use mrs_core::{MemoryAddress, Opcode, SkipCondition};
use std::collections::HashMap;

use crate::document::Document;
use crate::encoding::PositionEncoding;
use crate::features::diagnostics;

/// Offers fixes for the findings overlapping `range`.
pub fn code_actions(
    document: &Document,
    uri: &Uri,
    range: Range,
    encoding: PositionEncoding,
) -> Vec<CodeActionOrCommand> {
    let positions = document.positions(encoding);
    // Clamped rather than strict: a range that overshoots the document should still
    // offer the fixes it covers.
    let selection = positions.span_clamped(range);
    let published = diagnostics::diagnostics(document, uri, encoding);

    document
        .outcome
        .diagnostics
        .iter()
        .enumerate()
        // A selection touching the finding's span is what an editor sends when the
        // caret is on the squiggle.
        .filter(|(_, finding)| {
            finding.span.start <= selection.end && selection.start <= finding.span.end
        })
        .filter_map(|(index, finding)| {
            let (title, span, replacement) = fix(document, finding.code, finding.span)?;
            Some(action(
                title,
                uri,
                positions.range(span),
                replacement,
                published.get(index).cloned(),
            ))
        })
        .collect()
}

/// Works out the edit for one finding, if there is a mechanical one.
fn fix(document: &Document, code: Code, span: Span) -> Option<(String, Span, String)> {
    let assembly = document.assembly();
    let text = span.text(&document.text)?;

    match code {
        // An operand read as a label because it starts with a hex letter. Adding a
        // leading zero makes it the literal it was meant to be.
        Code::UNKNOWN_LABEL => {
            let hex = !text.is_empty() && text.chars().all(|c| c.is_ascii_hexdigit());
            let fits = text.trim_start_matches('0').len() <= 3;
            (hex && fits).then(|| {
                (
                    format!("Read `{text}` as the address `0{text}`"),
                    span,
                    format!("0{text}"),
                )
            })
        }

        // A Skipcond operand with bits that are silently ignored.
        code if code == crate::MASKED_SKIPCOND => {
            let operand = u16::from_str_radix(text.trim_start_matches('0'), 16).ok()?;
            let condition = SkipCondition::from_operand(MemoryAddress::new_masked(operand));
            let canonical = canonical_condition(condition);
            Some((
                format!("Change to `Skipcond {canonical}` ({condition})"),
                span,
                canonical,
            ))
        }

        // A mnemonic not in its canonical spelling.
        code if code == crate::NON_CANONICAL_MNEMONIC => {
            let item = assembly.program.item_at(span.start)?;
            let canonical = match item.mnemonic.kind {
                MnemonicKind::Opcode(opcode) => opcode.mnemonic(),
                MnemonicKind::Directive(directive) => directive.mnemonic(),
                MnemonicKind::Unknown => return None,
            };
            Some((
                format!("Write `{canonical}`"),
                item.mnemonic.span,
                canonical.to_owned(),
            ))
        }

        // A Skipcond given a label, which reads as a condition rather than a branch.
        code if code == crate::SKIPCOND_LABEL_OPERAND => {
            let item = assembly.program.item_at(span.start)?;
            (item.mnemonic.opcode() == Some(Opcode::SkipCond)).then(|| {
                (
                    format!("Branch to `{text}` with `Jump` instead"),
                    item.mnemonic.span,
                    "Jump".to_owned(),
                )
            })
        }

        _ => None,
    }
}

/// Formats a condition as an operand the assembler will read as a literal.
///
/// `C00` would be taken for a label, so it needs its leading zero.
fn canonical_condition(condition: SkipCondition) -> String {
    let operand = condition.to_operand().to_string();
    if operand.starts_with(|c: char| c.is_ascii_digit()) {
        operand
    } else {
        format!("0{operand}")
    }
}

/// Builds a quick-fix action for one edit.
// See the note on `navigation::rename` for why the `Uri` key is fine.
#[allow(clippy::mutable_key_type)]
fn action(
    title: String,
    uri: &Uri,
    range: Range,
    new_text: String,
    diagnostic: Option<Diagnostic>,
) -> CodeActionOrCommand {
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), vec![TextEdit { range, new_text }]);
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: diagnostic.map(|d| vec![d]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}
