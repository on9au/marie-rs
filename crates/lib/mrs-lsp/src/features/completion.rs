//! Completion for mnemonics, directives, labels and `Skipcond` conditions.

use lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, MarkupContent, MarkupKind, Position,
};
use mrs_core::{Directive, Opcode};

use crate::document::Document;
use crate::encoding::PositionEncoding;

/// Where in a line the caret sits, which decides what can go there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Context {
    /// Before any mnemonic: an instruction or directive belongs here.
    Mnemonic,
    /// After a mnemonic: an operand belongs here.
    Operand {
        /// The mnemonic already written, lowercased.
        mnemonic: String,
    },
    /// Inside a comment, where nothing is offered.
    Comment,
}

/// Works out what the caret can be followed by.
///
/// This reads the raw line rather than the syntax tree, because the line being typed is
/// usually not yet valid — which is the whole point of completion.
pub fn context_at(line: &str, column: usize) -> Context {
    let prefix: String = line.chars().take(column).collect();
    if prefix.contains('/') {
        return Context::Comment;
    }
    // A label and its comma are not part of the statement.
    let statement = match prefix.split_once(',') {
        Some((_, rest)) => rest,
        None => &prefix,
    };

    let mut words = statement.split_whitespace();
    let Some(first) = words.next() else {
        return Context::Mnemonic;
    };
    // Still typing the first word unless whitespace has closed it.
    let finished = statement.ends_with(char::is_whitespace) || words.next().is_some();
    if finished {
        Context::Operand {
            mnemonic: first.to_ascii_lowercase(),
        }
    } else {
        Context::Mnemonic
    }
}

/// Suggests what can be written at `position`.
pub fn completion(
    document: &Document,
    position: Position,
    encoding: PositionEncoding,
) -> Vec<CompletionItem> {
    let Some(line_span) = document.lines().line_span(position.line) else {
        return Vec::new();
    };
    let line = line_span.text(&document.text).unwrap_or_default();
    let positions = document.positions(encoding);
    let column = positions
        .offset(position)
        .map(|offset| (offset - line_span.start) as usize)
        .unwrap_or(0);
    // `context_at` counts characters, and the column is a byte offset into the line.
    let column = line[..column.min(line.len())].chars().count();

    match context_at(line, column) {
        Context::Comment => Vec::new(),
        Context::Mnemonic => mnemonics(),
        Context::Operand { mnemonic } => operands(document, &mnemonic),
    }
}

/// Every instruction and directive.
fn mnemonics() -> Vec<CompletionItem> {
    let opcodes = Opcode::ALL.into_iter().map(|opcode| CompletionItem {
        label: opcode.mnemonic().to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(format!("opcode 0x{:X}", opcode.to_nibble())),
        documentation: Some(docs(opcode.description())),
        // Instructions sort above directives.
        sort_text: Some(format!("0{}", opcode.mnemonic())),
        ..CompletionItem::default()
    });
    let directives = Directive::ALL.into_iter().map(|directive| CompletionItem {
        label: directive.mnemonic().to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("assembler directive".to_owned()),
        documentation: Some(docs(directive.description())),
        sort_text: Some(format!("1{}", directive.mnemonic())),
        ..CompletionItem::default()
    });
    opcodes.chain(directives).collect()
}

/// Labels, plus the four conditions when the mnemonic is `Skipcond`.
fn operands(document: &Document, mnemonic: &str) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    if mnemonic == "skipcond" {
        // Offering these is the reliable cure for the `C00` trap: the suggested text
        // carries the leading zero, so it stays a literal instead of becoming a label.
        for (operand, meaning) in [
            ("000", "skip if AC < 0"),
            ("400", "skip if AC = 0"),
            ("800", "skip if AC > 0"),
            ("0C00", "skip if AC != 0"),
        ] {
            items.push(CompletionItem {
                label: operand.to_owned(),
                kind: Some(CompletionItemKind::CONSTANT),
                detail: Some(meaning.to_owned()),
                documentation: Some(docs(
                    "Only bits 11-10 of the operand select the condition. \
                     `0C00` needs its leading zero, or it is read as a label.",
                )),
                sort_text: Some(format!("0{operand}")),
                ..CompletionItem::default()
            });
        }
        return items;
    }

    for symbol in document.assembly().symbols.iter() {
        let detail = document
            .assembly()
            .program
            .item_at_address(symbol.address)
            .and_then(|item| item.span.text(&document.text))
            .map(|text| text.trim().to_owned());
        items.push(CompletionItem {
            label: symbol.name.clone(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(format!("address {}", symbol.address)),
            documentation: detail.map(docs),
            ..CompletionItem::default()
        });
    }
    items
}

/// Wraps a description as Markdown documentation.
fn docs(text: impl Into<String>) -> Documentation {
    Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: text.into(),
    })
}
