//! The queries a linter, formatter or language server needs.
//!
//! Each test is written as the editor feature it backs, so that a change which breaks
//! go-to-definition fails a test called after go-to-definition.

use mrs_asm::lexer::{TokenKind, tokenize};
use mrs_asm::{Code, Options, Position, Role, Severity, assemble, assemble_with};

/// The source used by most of these tests, with a label defined once and used twice.
const SOURCE: &str = "\
/ Add two numbers.
        Load  First
        Add   Second
        Store First
        Halt
First,  DEC 21
Second, DEC 21
";

/// Returns the byte offset of the `nth` occurrence of `needle`, zero-based.
fn offset_of(source: &str, needle: &str, nth: usize) -> u32 {
    source
        .match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| panic!("no occurrence {nth} of {needle:?}"))
        .0 as u32
}

#[test]
fn go_to_definition_resolves_a_reference_to_its_definition() {
    let assembly = assemble(SOURCE);
    // The caret sits on `First` in `Load First`.
    let caret = offset_of(SOURCE, "First", 0);
    let (symbol, role) = assembly.symbols.find_at(caret).expect("symbol under caret");
    assert_eq!(symbol.name, "First");
    assert_eq!(role, Role::Reference);

    // Jumping to the definition lands on the `First,` label line.
    let position = assembly.lines.position(symbol.definition.start);
    assert_eq!(position, Position::new(5, 0));
    assert_eq!(symbol.address.value(), 4);
}

#[test]
fn go_to_definition_works_from_the_definition_itself() {
    let assembly = assemble(SOURCE);
    let caret = offset_of(SOURCE, "First,", 0);
    let (symbol, role) = assembly.symbols.find_at(caret).unwrap();
    assert_eq!(symbol.name, "First");
    assert_eq!(role, Role::Definition);
}

#[test]
fn find_all_references_returns_every_use_in_source_order() {
    let assembly = assemble(SOURCE);
    let symbol = assembly.symbols.get("First").unwrap();
    assert_eq!(symbol.references.len(), 2, "Load First and Store First");
    let lines: Vec<_> = symbol
        .references
        .iter()
        .map(|span| assembly.lines.position(span.start).line)
        .collect();
    assert_eq!(lines, vec![1, 3]);
    // References are recorded in source order.
    assert!(symbol.references[0].start < symbol.references[1].start);
}

#[test]
fn rename_covers_the_definition_and_every_reference() {
    let assembly = assemble(SOURCE);
    let caret = offset_of(SOURCE, "Second", 0);
    let (symbol, spans) = assembly.symbols.rename_spans(caret).unwrap();
    assert_eq!(symbol.name, "Second");
    // One definition plus one use.
    assert_eq!(spans.len(), 2);
    for span in &spans {
        assert_eq!(span.text(SOURCE), Some("Second"));
    }

    // Applying the rename back-to-front keeps the earlier offsets valid.
    let mut renamed = SOURCE.to_owned();
    let mut ordered = spans.clone();
    ordered.sort_by_key(|span| std::cmp::Reverse(span.start));
    for span in ordered {
        renamed.replace_range(span.range(), "Addend");
    }
    let reassembled = assemble(&renamed);
    assert!(reassembled.succeeded());
    assert_eq!(reassembled.words, assemble(SOURCE).words);
}

#[test]
fn hover_describes_the_mnemonic_under_the_caret() {
    let assembly = assemble(SOURCE);
    // Occurrence 0 is inside the leading comment; occurrence 1 is the instruction.
    let caret = offset_of(SOURCE, "Add", 1);
    let item = assembly.program.item_at(caret).expect("item under caret");
    assert_eq!(item.mnemonic.text, "Add");
    assert_eq!(
        item.mnemonic.description(),
        Some("Add the value at address X to the AC (AC <- AC + M[X])")
    );
    assert_eq!(item.mnemonic.opcode(), Some(mrs_core::Opcode::Add));

    // A caret inside a comment belongs to no item, so hover shows nothing.
    let in_comment = offset_of(SOURCE, "Add", 0);
    assert!(assembly.program.item_at(in_comment).is_none());
}

#[test]
fn document_symbols_lists_every_label_with_its_address() {
    let assembly = assemble(SOURCE);
    let listed: Vec<_> = assembly
        .symbols
        .iter()
        .map(|s| (s.name.as_str(), s.address.value(), s.line))
        .collect();
    // Iteration is by name, so the order is deterministic.
    assert_eq!(listed, vec![("First", 4, 5), ("Second", 5, 6)]);
}

#[test]
fn a_debugger_maps_between_addresses_and_lines_in_both_directions() {
    let assembly = assemble(SOURCE);
    let address = |v: u16| mrs_core::MemoryAddress::new(v);

    // Address to line, for highlighting the instruction at the program counter.
    assert_eq!(assembly.source_map.line_for(address(0)), Some(1));
    assert_eq!(assembly.source_map.line_for(address(4)), Some(5));

    // Line to address, for setting a breakpoint from the gutter.
    assert_eq!(assembly.source_map.address_for(1), Some(address(0)));
    assert_eq!(assembly.source_map.address_for(3), Some(address(2)));

    // A breakpoint on the comment line slides down to the next real instruction.
    assert_eq!(assembly.source_map.address_for(0), None);
    assert_eq!(assembly.source_map.address_at_or_after(0), Some(address(0)));
}

#[test]
fn semantic_tokens_classify_every_part_of_a_line() {
    let source = "Start, Load 0FF / go\nORG 100\n";
    let kinds: Vec<_> = tokenize(source)
        .iter()
        .map(|t| (t.kind, t.span.text(source).unwrap()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (TokenKind::Label, "Start"),
            (TokenKind::Comma, ","),
            (TokenKind::Mnemonic, "Load"),
            (TokenKind::Operand, "0FF"),
            (TokenKind::Comment, "/ go"),
            (TokenKind::OriginKeyword, "ORG"),
            (TokenKind::Operand, "100"),
        ]
    );
}

#[test]
fn tokens_are_produced_even_for_a_file_that_does_not_assemble() {
    // An editor must still colour broken source.
    let source = "Foo, Load X Y\n???\n";
    let tokens = tokenize(source);
    assert!(!tokens.is_empty());
    assert!(tokens.iter().any(|t| t.kind == TokenKind::Unknown));
}

#[test]
fn diagnostics_carry_spans_that_point_at_the_offending_text() {
    let source = "Load Missing\n";
    let assembly = assemble(source);
    let diagnostic = assembly.errors().next().unwrap();
    assert_eq!(diagnostic.code, Code::UNKNOWN_LABEL);
    assert_eq!(diagnostic.span.text(source), Some("Missing"));
    assert_eq!(
        assembly.lines.position(diagnostic.span.start),
        Position::new(0, 5)
    );
}

#[test]
fn every_diagnostic_can_be_sorted_into_source_order() {
    // The raw order follows MARIE.js's two passes, so a parse error on a late line
    // comes before an assembly error on an early one.
    let source = "Load Missing\n1Bad, Halt\n";
    let assembly = assemble(source);
    let raw: Vec<_> = assembly.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        raw,
        vec![Code::LABEL_STARTS_WITH_DIGIT, Code::UNKNOWN_LABEL]
    );

    let sorted: Vec<_> = assembly
        .diagnostics_in_source_order()
        .iter()
        .map(|d| d.code)
        .collect();
    assert_eq!(
        sorted,
        vec![Code::UNKNOWN_LABEL, Code::LABEL_STARTS_WITH_DIGIT]
    );
}

#[test]
fn a_partial_program_is_still_returned_for_a_file_with_errors() {
    // Every item yields a word, so the AST, the word list and the source map stay
    // aligned even when nothing assembles.
    let assembly = assemble("Load Missing\nHalt\n");
    assert!(!assembly.succeeded());
    assert_eq!(assembly.program.items.len(), 2);
    assert_eq!(assembly.words.len(), 2);
    assert_eq!(assembly.words[0], 0, "the failed item still emits a word");
    assert_eq!(assembly.words[1] as u16, 0x7000);
    assert_eq!(assembly.source_map.len(), 2);
    // An image is refused while errors stand.
    assert!(assembly.image().is_none());
}

#[test]
fn lints_are_off_by_default_and_never_block_assembly() {
    let source = "Unused, DEC 1\nHalt\nEND\nleftovers\n";

    let plain = assemble(source);
    assert!(plain.succeeded());
    assert_eq!(plain.warnings().count(), 0, "no warnings without lint");

    let linted = assemble_with(source, Options::linting());
    assert!(linted.succeeded(), "warnings must not fail assembly");
    let codes: Vec<_> = linted.warnings().map(|d| d.code).collect();
    assert!(codes.contains(&Code::UNUSED_LABEL));
    assert!(codes.contains(&Code::UNREACHABLE_CODE));
    assert!(linted.warnings().all(|d| d.severity == Severity::Warning));
    // The words are identical either way.
    assert_eq!(plain.words, linted.words);
}

#[test]
fn the_ignored_operand_lint_catches_a_pointless_clear_operand() {
    let linted = assemble_with("Clear 5", Options::linting());
    assert!(linted.succeeded());
    assert_eq!(
        linted.warnings().map(|d| d.code).collect::<Vec<_>>(),
        vec![Code::IGNORED_OPERAND]
    );
}

#[test]
fn an_image_loads_at_the_origin_and_leaves_the_rest_of_memory_alone() {
    let assembly = assemble("ORG 010\nHalt\nDEC 5\n");
    let image = assembly.image().expect("assembles and fits");
    assert_eq!(image[0x010], 0x7000_u16 as i16);
    assert_eq!(image[0x011], 5);
    assert_eq!(image[0x00F], 0);
    assert_eq!(image[0x012], 0);
}

#[test]
fn positions_and_offsets_agree_across_the_whole_file() {
    let assembly = assemble(SOURCE);
    for offset in 0..=SOURCE.len() as u32 {
        let position = assembly.lines.position(offset);
        assert_eq!(assembly.lines.offset(position), Some(offset));
    }
}
