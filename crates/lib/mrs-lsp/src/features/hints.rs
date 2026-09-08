//! Inlay hints showing where each line lands and what it assembles to.
//!
//! MARIE is taught alongside a memory view, and the question a reader keeps asking is
//! "which address is this, and what word does it become?". Both are known after
//! assembly, so the editor can simply say.

use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Range};

use crate::document::Document;
use crate::encoding::PositionEncoding;

/// Builds a hint for every assembled word whose line falls inside `range`.
pub fn inlay_hints(
    document: &Document,
    range: Range,
    encoding: PositionEncoding,
) -> Vec<InlayHint> {
    let positions = document.positions(encoding);
    let assembly = document.assembly();
    // Only errors make the words meaningless; warnings are fine.
    if !assembly.succeeded() {
        return Vec::new();
    }

    assembly
        .program
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.line >= range.start.line && item.line <= range.end.line)
        .filter_map(|(index, item)| {
            let word = *assembly.words.get(index)? as u16;
            // Sit at the end of the source line so the hint never splits the code.
            let line_end = document.lines().line_end(item.line)?;
            Some(InlayHint {
                position: positions.position(line_end),
                label: InlayHintLabel::String(format!("{}: {word:04X}", item.address)),
                kind: Some(InlayHintKind::PARAMETER),
                text_edits: None,
                tooltip: None,
                padding_left: Some(true),
                padding_right: None,
                data: None,
            })
        })
        .collect()
}
