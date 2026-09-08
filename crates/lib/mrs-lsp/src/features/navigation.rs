//! Go to definition, find references, highlight and rename.
//!
//! All four are the same query — resolve the caret to a label — differing only in what
//! they return, so they share [`crate::features::navigation::symbol_at`].

use lsp_types::{
    DocumentHighlight, DocumentHighlightKind, Location, Position, PrepareRenameResponse, Range,
    TextEdit, Uri, WorkspaceEdit,
};
use mrs_asm::symbols::{Role, Symbol};
use std::collections::HashMap;

use crate::document::Document;
use crate::encoding::PositionEncoding;

/// Resolves a position to the label written there, if any.
pub fn symbol_at(
    document: &Document,
    position: Position,
    encoding: PositionEncoding,
) -> Option<(&Symbol, Role)> {
    let positions = document.positions(encoding);
    let offset = positions.offset(position)?;
    document.assembly().symbols.find_at(offset)
}

/// The definition site of the label under the caret.
pub fn definition(
    document: &Document,
    uri: &Uri,
    position: Position,
    encoding: PositionEncoding,
) -> Option<Location> {
    let (symbol, _) = symbol_at(document, position, encoding)?;
    let positions = document.positions(encoding);
    Some(Location::new(
        uri.clone(),
        positions.range(symbol.definition),
    ))
}

/// Every use of the label under the caret.
pub fn references(
    document: &Document,
    uri: &Uri,
    position: Position,
    encoding: PositionEncoding,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let (symbol, _) = symbol_at(document, position, encoding)?;
    let positions = document.positions(encoding);
    let mut locations = Vec::with_capacity(symbol.references.len() + 1);
    if include_declaration {
        locations.push(Location::new(
            uri.clone(),
            positions.range(symbol.definition),
        ));
    }
    locations.extend(
        symbol
            .references
            .iter()
            .map(|span| Location::new(uri.clone(), positions.range(*span))),
    );
    Some(locations)
}

/// Highlights the label under the caret throughout the file.
pub fn highlight(
    document: &Document,
    position: Position,
    encoding: PositionEncoding,
) -> Option<Vec<DocumentHighlight>> {
    let (symbol, _) = symbol_at(document, position, encoding)?;
    let positions = document.positions(encoding);
    let mut highlights = vec![DocumentHighlight {
        range: positions.range(symbol.definition),
        // The definition is where the name is written, which editors show as a write.
        kind: Some(DocumentHighlightKind::WRITE),
    }];
    highlights.extend(symbol.references.iter().map(|span| DocumentHighlight {
        range: positions.range(*span),
        kind: Some(DocumentHighlightKind::READ),
    }));
    Some(highlights)
}

/// Why a label cannot be renamed to a given string.
///
/// These are the assembler's own rules, so a rename the server accepts is one the
/// assembler will accept too.
pub fn rename_error(new_name: &str) -> Option<&'static str> {
    if new_name.is_empty() {
        return Some("a label cannot be empty");
    }
    if new_name.starts_with(|c: char| c.is_ascii_digit()) {
        return Some("a label cannot start with a digit");
    }
    if new_name.contains(char::is_whitespace) {
        return Some("a label cannot contain whitespace");
    }
    if new_name.contains(',') {
        return Some("a label cannot contain a comma");
    }
    if new_name.contains('/') {
        return Some("a label cannot contain a slash, which starts a comment");
    }
    None
}

/// Confirms that the caret is on something renameable, and says what will be replaced.
pub fn prepare_rename(
    document: &Document,
    position: Position,
    encoding: PositionEncoding,
) -> Option<PrepareRenameResponse> {
    let (symbol, role) = symbol_at(document, position, encoding)?;
    let positions = document.positions(encoding);
    let span = match role {
        Role::Definition => symbol.definition,
        Role::Reference => *symbol
            .references
            .iter()
            .find(|span| {
                positions
                    .offset(position)
                    .is_some_and(|offset| span.touches(offset))
            })
            .unwrap_or(&symbol.definition),
    };
    Some(PrepareRenameResponse::RangeWithPlaceholder {
        range: positions.range(span),
        placeholder: symbol.name.clone(),
    })
}

/// Renames the label under the caret, definition and every reference.
// `WorkspaceEdit::changes` is keyed by `Uri`, which clippy flags because `fluent_uri`
// holds a `Cell` internally. The protocol dictates the type, and the keys are never
// mutated, so the lint has nothing to catch here.
#[allow(clippy::mutable_key_type)]
pub fn rename(
    document: &Document,
    uri: &Uri,
    position: Position,
    new_name: &str,
    encoding: PositionEncoding,
) -> Result<Option<WorkspaceEdit>, &'static str> {
    if let Some(reason) = rename_error(new_name) {
        return Err(reason);
    }
    let Some((symbol, _)) = symbol_at(document, position, encoding) else {
        return Ok(None);
    };
    // Renaming to a name that already exists would silently merge two labels.
    if symbol.name != new_name && document.assembly().symbols.contains(new_name) {
        return Err("a label with that name already exists");
    }

    let positions = document.positions(encoding);
    let edits: Vec<TextEdit> = std::iter::once(symbol.definition)
        .chain(symbol.references.iter().copied())
        .map(|span| TextEdit {
            range: positions.range(span),
            new_text: new_name.to_owned(),
        })
        .collect();

    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), edits);
    Ok(Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    }))
}

/// The range of the label under the caret, for tests and callers that want it.
pub fn symbol_range(
    document: &Document,
    position: Position,
    encoding: PositionEncoding,
) -> Option<Range> {
    let (symbol, _) = symbol_at(document, position, encoding)?;
    Some(document.positions(encoding).range(symbol.definition))
}
