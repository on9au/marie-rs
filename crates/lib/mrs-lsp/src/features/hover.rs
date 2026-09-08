//! Hover: what is under the caret, and what it assembles to.

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};
use mrs_asm::ast::{Item, MnemonicKind};
use mrs_core::Instruction;

use crate::document::Document;
use crate::encoding::PositionEncoding;

/// Describes whatever the caret is on.
pub fn hover(document: &Document, position: Position, encoding: PositionEncoding) -> Option<Hover> {
    let positions = document.positions(encoding);
    let offset = positions.offset(position)?;
    let assembly = document.assembly();

    // A label is worth more than the line it sits on, so it wins.
    if let Some((symbol, _)) = assembly.symbols.find_at(offset) {
        let mut text = format!("**{}** — label at `{}`\n\n", symbol.name, symbol.address);
        if let Some(item) = assembly.program.item_at_address(symbol.address) {
            text.push_str(&describe_word(assembly, item));
        }
        let uses = symbol.references.len();
        text.push_str(&format!(
            "\n\n*{uses} reference{}*",
            if uses == 1 { "" } else { "s" }
        ));
        return Some(markdown(text, positions.range(symbol.definition)));
    }

    let item = assembly.program.item_at(offset)?;

    // On the mnemonic: what the instruction does.
    if item.mnemonic.span.touches(offset) {
        let mut text = match item.mnemonic.kind {
            MnemonicKind::Opcode(opcode) => format!(
                "**{}** — opcode `0x{:X}`\n\n{}",
                opcode.mnemonic(),
                opcode.to_nibble(),
                opcode.description()
            ),
            MnemonicKind::Directive(directive) => format!(
                "**{}** — assembler directive\n\n{}",
                directive.mnemonic(),
                directive.description()
            ),
            MnemonicKind::Unknown => format!("`{}` is not a known mnemonic", item.mnemonic.text),
        };
        text.push_str("\n\n---\n\n");
        text.push_str(&describe_word(assembly, item));
        return Some(markdown(text, positions.range(item.mnemonic.span)));
    }

    // On a literal operand: the value in every base a MARIE programmer reads.
    if let Some(operand) = &item.operand
        && operand.span.touches(offset)
        && operand.looks_like_address_literal()
    {
        let value = u16::from_str_radix(operand.text.trim_start_matches('0'), 16).unwrap_or(0);
        let text = format!(
            "`{}` is **hexadecimal**: `0x{value:03X}` = {value} decimal\n\n\
             Instruction operands are always hexadecimal, whatever the nearby directives use.",
            operand.text
        );
        return Some(markdown(text, positions.range(operand.span)));
    }

    None
}

/// The address an item occupies and the word it assembles to.
fn describe_word(assembly: &mrs_asm::Assembly, item: &Item) -> String {
    let Some(index) = assembly
        .program
        .items
        .iter()
        .position(|candidate| candidate.address == item.address)
    else {
        return String::new();
    };
    let Some(word) = assembly.words.get(index) else {
        return String::new();
    };
    let bits = *word as u16;
    let mut text = format!(
        "Address `{}` — word `0x{bits:04X}` ({} decimal)",
        item.address, word
    );
    if let Some(instruction) = Instruction::decode(mrs_core::Value::new(*word)) {
        text.push_str(&format!("\n\nDecodes as `{instruction}`"));
    }
    text
}

/// Wraps text as a Markdown hover over `range`.
fn markdown(text: String, range: lsp_types::Range) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: text,
        }),
        range: Some(range),
    }
}
