//! Semantic tokens for syntax highlighting.
//!
//! The lexer already segments a line, so this only classifies and delta-encodes. Two
//! details are easy to get wrong: token lengths are measured in the *negotiated
//! encoding*, not in bytes, and a token may never span a line break.

use lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
};
use mrs_asm::lexer::{TokenKind, tokenize};

use crate::document::Document;
use crate::encoding::PositionEncoding;

/// The token types this server emits, in the order the legend indexes them.
const TYPES: [SemanticTokenType; 6] = [
    SemanticTokenType::KEYWORD,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::NUMBER,
    SemanticTokenType::COMMENT,
    SemanticTokenType::OPERATOR,
];

const KEYWORD: u32 = 0;
const FUNCTION: u32 = 1;
const VARIABLE: u32 = 2;
const NUMBER: u32 = 3;
const COMMENT: u32 = 4;
const OPERATOR: u32 = 5;

/// The one modifier used: a label's own definition.
const DECLARATION: u32 = 1 << 0;

/// The legend the client is told about during initialisation.
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TYPES.to_vec(),
        token_modifiers: vec![SemanticTokenModifier::DECLARATION],
    }
}

/// Classifies and delta-encodes every token in the document.
pub fn semantic_tokens(document: &Document, encoding: PositionEncoding) -> SemanticTokens {
    let positions = document.positions(encoding);
    let symbols = &document.assembly().symbols;

    let mut data = Vec::new();
    let mut previous_line = 0u32;
    let mut previous_start = 0u32;

    for token in tokenize(&document.text) {
        let (kind, modifiers) = match token.kind {
            TokenKind::Label => (FUNCTION, DECLARATION),
            TokenKind::Comma => (OPERATOR, 0),
            TokenKind::OriginKeyword | TokenKind::Mnemonic => (KEYWORD, 0),
            TokenKind::Operand => {
                let text = token.span.text(&document.text).unwrap_or_default();
                // The assembler's own rule: a leading decimal digit makes it a literal.
                if text.starts_with(|c: char| c.is_ascii_digit()) {
                    (NUMBER, 0)
                } else if symbols.contains(text) {
                    (FUNCTION, 0)
                } else {
                    (VARIABLE, 0)
                }
            }
            TokenKind::Comment => (COMMENT, 0),
            // Unclassifiable text is left for the client's own highlighting.
            TokenKind::Unknown => continue,
        };

        let start = positions.position(token.span.start);
        let length = positions.length(token.span);
        if length == 0 {
            continue;
        }

        // Positions are relative to the previous token, and to the line start whenever
        // the line changes.
        let delta_line = start.line - previous_line;
        let delta_start = if delta_line == 0 {
            start.character - previous_start
        } else {
            start.character
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: kind,
            token_modifiers_bitset: modifiers,
        });
        previous_line = start.line;
        previous_start = start.character;
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}
