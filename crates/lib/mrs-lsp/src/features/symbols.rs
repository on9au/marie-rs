//! The document outline.

use lsp_types::{DocumentSymbol, SymbolKind};
use mrs_asm::ast::MnemonicKind;
use mrs_core::Directive;

use crate::document::Document;
use crate::encoding::PositionEncoding;

/// Lists every label, ordered by address so the outline follows the program.
pub fn document_symbols(document: &Document, encoding: PositionEncoding) -> Vec<DocumentSymbol> {
    let positions = document.positions(encoding);
    let assembly = document.assembly();

    let mut symbols: Vec<_> = assembly.symbols.iter().collect();
    symbols.sort_by_key(|symbol| symbol.address);

    symbols
        .into_iter()
        .map(|symbol| {
            let item = assembly.program.item_at_address(symbol.address);
            // A label on a data word is a variable; one on an instruction names code.
            let kind = match item.map(|item| item.mnemonic.kind) {
                Some(MnemonicKind::Directive(
                    Directive::Dec | Directive::Oct | Directive::Hex | Directive::Adr,
                )) => SymbolKind::VARIABLE,
                _ => SymbolKind::FUNCTION,
            };
            let detail = item.map(|item| {
                format!(
                    "{} — {}",
                    symbol.address,
                    item.span.text(&document.text).unwrap_or_default().trim()
                )
            });
            let full = item.map_or(symbol.definition, |item| symbol.definition.join(item.span));

            #[allow(deprecated)] // `deprecated` is required by the struct, not by us.
            DocumentSymbol {
                name: symbol.name.clone(),
                detail,
                kind,
                tags: None,
                deprecated: None,
                range: positions.range(full),
                selection_range: positions.range(symbol.definition),
                children: None,
            }
        })
        .collect()
}
