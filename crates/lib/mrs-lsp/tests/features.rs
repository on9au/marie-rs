//! The language features, driven the way an editor drives them.

use std::str::FromStr;

use lsp_types::{
    CodeActionOrCommand, DocumentHighlightKind, HoverContents, InlayHintLabel, MarkupContent,
    Position, PrepareRenameResponse, Range, SymbolKind, Uri,
};
use mrs_lint::Linter;
use mrs_lsp::features::{
    actions, completion, diagnostics, hints, hover, navigation, symbols, tokens,
};
use mrs_lsp::{Document, PositionEncoding};

/// A program with a label used twice, a data word, and a comment.
const SOURCE: &str = "\
/ Add two numbers.
        Load  First
        Add   Second
        Store First
        Halt
First,  DEC 21
Second, DEC 21
";

fn uri() -> Uri {
    Uri::from_str("file:///test.mas").unwrap()
}

fn document(source: &str) -> Document {
    Document::new(source.to_owned(), 1, &Linter::new())
}

/// The position of the `nth` occurrence of `needle`, zero-based.
fn at(source: &str, needle: &str, nth: usize) -> Position {
    let offset = source
        .match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| panic!("no occurrence {nth} of {needle:?}"))
        .0;
    let line = source[..offset].matches('\n').count() as u32;
    let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    Position::new(line, (offset - line_start) as u32)
}

const UTF16: PositionEncoding = PositionEncoding::Utf16;

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

#[test]
fn go_to_definition_jumps_from_a_use_to_the_label() {
    let document = document(SOURCE);
    let location = navigation::definition(&document, &uri(), at(SOURCE, "First", 0), UTF16)
        .expect("a definition");
    // `First,` is on line 5.
    assert_eq!(location.range.start, Position::new(5, 0));
    assert_eq!(location.range.end, Position::new(5, 5));
}

#[test]
fn go_to_definition_on_a_comment_finds_nothing() {
    let document = document(SOURCE);
    assert!(navigation::definition(&document, &uri(), Position::new(0, 4), UTF16).is_none());
}

#[test]
fn find_references_returns_both_uses_and_optionally_the_declaration() {
    let document = document(SOURCE);
    let without =
        navigation::references(&document, &uri(), at(SOURCE, "First", 0), UTF16, false).unwrap();
    assert_eq!(without.len(), 2, "Load First and Store First");

    let with =
        navigation::references(&document, &uri(), at(SOURCE, "First", 0), UTF16, true).unwrap();
    assert_eq!(with.len(), 3);
    assert_eq!(
        with[0].range.start,
        Position::new(5, 0),
        "declaration first"
    );
}

#[test]
fn highlight_marks_the_definition_as_a_write_and_uses_as_reads() {
    let document = document(SOURCE);
    let highlights = navigation::highlight(&document, at(SOURCE, "First", 0), UTF16).unwrap();
    assert_eq!(highlights[0].kind, Some(DocumentHighlightKind::WRITE));
    assert!(
        highlights[1..]
            .iter()
            .all(|h| h.kind == Some(DocumentHighlightKind::READ))
    );
}

#[test]
fn prepare_rename_offers_the_current_name_as_the_placeholder() {
    let document = document(SOURCE);
    let response = navigation::prepare_rename(&document, at(SOURCE, "Second", 0), UTF16).unwrap();
    let PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. } = response else {
        panic!("expected a placeholder");
    };
    assert_eq!(placeholder, "Second");
}

#[test]
fn rename_edits_the_definition_and_every_reference() {
    let document = document(SOURCE);
    let edit = navigation::rename(&document, &uri(), at(SOURCE, "First", 0), "Total", UTF16)
        .unwrap()
        .unwrap();
    let edits = &edit.changes.unwrap()[&uri()];
    assert_eq!(edits.len(), 3, "one definition and two uses");
    assert!(edits.iter().all(|e| e.new_text == "Total"));
}

#[test]
fn rename_rejects_names_the_assembler_would_reject() {
    let document = document(SOURCE);
    for (name, reason) in [
        ("1Bad", "digit"),
        ("has space", "whitespace"),
        ("comma,", "comma"),
        ("slash/", "slash"),
        ("", "empty"),
    ] {
        let result = navigation::rename(&document, &uri(), at(SOURCE, "First", 0), name, UTF16);
        assert!(result.is_err(), "{name} should be rejected ({reason})");
    }
}

#[test]
fn rename_refuses_to_merge_two_labels() {
    let document = document(SOURCE);
    let result = navigation::rename(&document, &uri(), at(SOURCE, "First", 0), "Second", UTF16);
    assert!(result.is_err(), "renaming onto an existing label must fail");
}

// ---------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------

/// The markdown of a hover, for substring assertions.
fn hover_text(document: &Document, position: Position) -> String {
    let hover = hover::hover(document, position, UTF16).expect("a hover");
    let HoverContents::Markup(MarkupContent { value, .. }) = hover.contents else {
        panic!("expected markup");
    };
    value
}

#[test]
fn hovering_a_mnemonic_describes_the_instruction_and_the_word() {
    let document = document(SOURCE);
    // Occurrence 0 of "Add" is in the comment, so take the next one.
    let text = hover_text(&document, at(SOURCE, "Add", 1));
    assert!(text.contains("**Add**"), "{text}");
    assert!(text.contains("0x3"), "names the opcode:\n{text}");
    assert!(text.contains("AC <- AC + M[X]"), "{text}");
    assert!(text.contains("0x3005"), "shows the assembled word:\n{text}");
}

#[test]
fn hovering_a_label_shows_its_address_and_contents() {
    let document = document(SOURCE);
    let text = hover_text(&document, at(SOURCE, "First", 0));
    assert!(text.contains("**First**"), "{text}");
    assert!(text.contains("004"), "shows the address:\n{text}");
    assert!(text.contains("2 references"), "counts uses:\n{text}");
}

#[test]
fn hovering_a_literal_operand_says_it_is_hexadecimal() {
    let document = document("        Add 123\n");
    let text = hover_text(&document, Position::new(0, 12));
    assert!(text.contains("hexadecimal"), "{text}");
    assert!(text.contains("291"), "gives the decimal value:\n{text}");
}

// ---------------------------------------------------------------------------
// Symbols, completion
// ---------------------------------------------------------------------------

#[test]
fn document_symbols_are_ordered_by_address_and_typed_by_content() {
    let document = document(SOURCE);
    let listed = symbols::document_symbols(&document, UTF16);
    let names: Vec<_> = listed.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["First", "Second"]);
    // Both label data words.
    assert!(listed.iter().all(|s| s.kind == SymbolKind::VARIABLE));

    let on_code = self::document("Start,  Halt\n");
    let listed = symbols::document_symbols(&on_code, UTF16);
    assert_eq!(listed[0].kind, SymbolKind::FUNCTION, "a label on code");
}

#[test]
fn completion_offers_mnemonics_in_the_mnemonic_position() {
    let document = document("        \n");
    let items = completion::completion(&document, Position::new(0, 8), UTF16);
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"Load"));
    assert!(labels.contains(&"Halt"));
    assert!(labels.contains(&"DEC"), "directives too");
}

#[test]
fn completion_offers_labels_in_the_operand_position() {
    let source = "        Load  \nFirst,  DEC 1\n";
    let document = document(source);
    let items = completion::completion(&document, Position::new(0, 14), UTF16);
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"First"), "got {labels:?}");
    assert!(!labels.contains(&"Load"), "not mnemonics: {labels:?}");
}

#[test]
fn completion_offers_the_four_conditions_after_skipcond() {
    let document = document("        Skipcond \n");
    let items = completion::completion(&document, Position::new(0, 17), UTF16);
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, vec!["000", "400", "800", "0C00"]);
    // The suggestion carries the leading zero, which is what keeps it a literal.
    assert!(labels.contains(&"0C00"), "and never bare C00");
}

#[test]
fn completion_is_silent_inside_a_comment() {
    let document = document("        Load X / here\n");
    let items = completion::completion(&document, Position::new(0, 19), UTF16);
    assert!(items.is_empty());
}

// ---------------------------------------------------------------------------
// Semantic tokens and inlay hints
// ---------------------------------------------------------------------------

#[test]
fn semantic_tokens_decode_back_to_the_right_absolute_positions() {
    // Delta encoding is easy to get subtly wrong, so decode it and check.
    let source = "First,  Load  First / c\n        Halt\n";
    let document = document(source);
    let encoded = tokens::semantic_tokens(&document, UTF16);

    let mut line = 0u32;
    let mut start = 0u32;
    let mut decoded = Vec::new();
    for token in &encoded.data {
        line += token.delta_line;
        start = if token.delta_line == 0 {
            start + token.delta_start
        } else {
            token.delta_start
        };
        decoded.push((line, start, token.length, token.token_type));
    }

    assert_eq!(
        decoded,
        vec![
            (0, 0, 5, 1),  // First   -> FUNCTION
            (0, 5, 1, 5),  // ,       -> OPERATOR
            (0, 8, 4, 0),  // Load    -> KEYWORD
            (0, 14, 5, 1), // First   -> FUNCTION (a known label)
            (0, 20, 3, 4), // / c     -> COMMENT
            (1, 8, 4, 0),  // Halt    -> KEYWORD
        ]
    );
}

#[test]
fn token_lengths_are_measured_in_the_negotiated_encoding() {
    // `café` is five bytes but four UTF-16 units.
    let source = "caf\u{e9},  Halt\n";
    let document = document(source);
    let utf16 = tokens::semantic_tokens(&document, PositionEncoding::Utf16);
    let utf8 = tokens::semantic_tokens(&document, PositionEncoding::Utf8);
    assert_eq!(utf16.data[0].length, 4);
    assert_eq!(utf8.data[0].length, 5);
}

#[test]
fn inlay_hints_show_each_line_s_address_and_word() {
    let document = document(SOURCE);
    let all = Range::new(Position::new(0, 0), Position::new(99, 0));
    let hints = hints::inlay_hints(&document, all, UTF16);
    let labels: Vec<_> = hints
        .iter()
        .map(|hint| match &hint.label {
            InlayHintLabel::String(text) => text.clone(),
            InlayHintLabel::LabelParts(_) => unreachable!("only strings are produced"),
        })
        .collect();
    assert_eq!(
        labels,
        vec![
            "000: 1004", // Load First
            "001: 3005", // Add Second
            "002: 2004", // Store First
            "003: 7000", // Halt
            "004: 0015", // DEC 21
            "005: 0015", // DEC 21
        ]
    );
}

#[test]
fn inlay_hints_are_limited_to_the_requested_range() {
    let document = document(SOURCE);
    let hints = hints::inlay_hints(
        &document,
        Range::new(Position::new(3, 0), Position::new(4, 0)),
        UTF16,
    );
    assert_eq!(hints.len(), 2);
}

#[test]
fn no_inlay_hints_are_offered_for_a_file_that_does_not_assemble() {
    // The words would be meaningless, and wrong hints are worse than none.
    let document = document("        Load  Missing\n");
    let all = Range::new(Position::new(0, 0), Position::new(99, 0));
    assert!(hints::inlay_hints(&document, all, UTF16).is_empty());
}

// ---------------------------------------------------------------------------
// Diagnostics and quick fixes
// ---------------------------------------------------------------------------

#[test]
fn diagnostics_carry_the_stable_code_and_the_help_text() {
    let document = document("        Load  Missing\n");
    let published = diagnostics::diagnostics(&document, &uri(), UTF16);
    let diagnostic = published
        .iter()
        .find(|d| {
            d.code
                == Some(lsp_types::NumberOrString::String(
                    "asm::unknown-label".to_owned(),
                ))
        })
        .expect("the unknown-label diagnostic");
    assert_eq!(diagnostic.source.as_deref(), Some("asm"));
    assert!(
        diagnostic.message.contains("help:"),
        "{}",
        diagnostic.message
    );
    assert_eq!(diagnostic.range.start, Position::new(0, 14));
}

#[test]
fn a_secondary_label_becomes_related_information() {
    let document = document("Dup, DEC 1\nDup, DEC 2\n");
    let published = diagnostics::diagnostics(&document, &uri(), UTF16);
    let related = published[0].related_information.as_ref().unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].location.range.start, Position::new(0, 0));
}

#[test]
fn advice_is_published_as_a_hint() {
    // `self-modifying-code` is advice, which an editor should not show as a problem.
    let source = "        Load  Patch\n        Store Victim\nVictim, Output\n        Halt\nPatch,  HEX 7000\n";
    let document = document(source);
    let published = diagnostics::diagnostics(&document, &uri(), UTF16);
    let advice = published
        .iter()
        .find(|d| {
            d.code
                == Some(lsp_types::NumberOrString::String(
                    "lint::self-modifying-code".to_owned(),
                ))
        })
        .expect("the advice");
    assert_eq!(advice.severity, Some(lsp_types::DiagnosticSeverity::HINT));
}

/// The titles of the quick fixes offered over a whole document.
fn fix_titles(source: &str) -> Vec<String> {
    let document = document(source);
    let all = Range::new(Position::new(0, 0), Position::new(99, 0));
    actions::code_actions(&document, &uri(), all, UTF16)
        .into_iter()
        .map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => action.title,
            CodeActionOrCommand::Command(command) => command.title,
        })
        .collect()
}

#[test]
fn a_hex_operand_read_as_a_label_can_be_fixed_with_a_leading_zero() {
    let titles = fix_titles("        Skipcond C00\n        Halt\n");
    assert!(
        titles.iter().any(|t| t.contains("`0C00`")),
        "got {titles:?}"
    );
}

#[test]
fn the_leading_zero_fix_produces_source_that_assembles() {
    let source = "        Skipcond C00\n        Halt\n";
    let document = document(source);
    let all = Range::new(Position::new(0, 0), Position::new(99, 0));
    let CodeActionOrCommand::CodeAction(action) =
        actions::code_actions(&document, &uri(), all, UTF16)
            .into_iter()
            .next()
            .expect("a fix")
    else {
        panic!("expected an action");
    };

    // Apply the edit and check the result really assembles now.
    let edits = &action.edit.unwrap().changes.unwrap()[&uri()];
    let edit = &edits[0];
    let lines = mrs_asm::span::LineIndex::new(source);
    let start = lines.line_start(edit.range.start.line).unwrap() + edit.range.start.character;
    let end = lines.line_start(edit.range.end.line).unwrap() + edit.range.end.character;
    let mut fixed = source.to_owned();
    fixed.replace_range(start as usize..end as usize, &edit.new_text);

    let assembly = mrs_asm::assemble(&fixed);
    assert!(assembly.succeeded(), "{fixed:?}");
    assert_eq!(assembly.words[0] as u16, 0x8C00, "the non-zero condition");
}

#[test]
fn a_masked_skipcond_can_be_fixed_to_the_condition_it_actually_tests() {
    let titles = fix_titles("        Load One\n        Skipcond 401\n        Halt\nOne, DEC 1\n");
    assert!(
        titles.iter().any(|t| t.contains("Skipcond 400")),
        "got {titles:?}"
    );
}

#[test]
fn a_shouted_mnemonic_can_be_rewritten() {
    let titles = fix_titles("        HALT\n");
    assert!(titles.iter().any(|t| t == "Write `Halt`"), "got {titles:?}");
}

#[test]
fn no_fixes_are_offered_for_a_clean_file() {
    let titles = fix_titles("        Load  X\n        Halt\nX,      DEC 1\n");
    assert!(titles.is_empty(), "got {titles:?}");
}
