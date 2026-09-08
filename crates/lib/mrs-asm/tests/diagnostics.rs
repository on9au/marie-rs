//! Every diagnostic the assembler can produce, and the guarantee that it never panics.
//!
//! A language server calls the assembler on every keystroke, which means it is
//! constantly handed text that is halfway through being typed. The robustness tests at
//! the bottom exist because "assembling nonsense returns errors" is a much stronger
//! contract than "assembling valid programs works".

use mrs_asm::{Code, Options, assemble, assemble_with};

/// The error codes a source file produces, in order.
fn codes(source: &str) -> Vec<Code> {
    assemble(source).errors().map(|d| d.code).collect()
}

/// The first error message a source file produces.
fn message(source: &str) -> String {
    assemble(source)
        .errors()
        .next()
        .map(|d| d.message.clone())
        .unwrap_or_else(|| panic!("expected an error for {source:?}"))
}

#[test]
fn malformed_line() {
    // Two operands, a comma with nothing before it, and a label with no instruction.
    assert_eq!(codes("Load X Y"), vec![Code::MALFORMED_LINE]);
    assert_eq!(codes(", Halt"), vec![Code::MALFORMED_LINE]);
    assert_eq!(codes("Foo,"), vec![Code::MALFORMED_LINE]);
    assert_eq!(message("Load X Y"), "Line has incorrect form.");
}

#[test]
fn unexpected_origin() {
    assert_eq!(codes("Halt\nORG 100"), vec![Code::UNEXPECTED_ORIGIN]);
    assert_eq!(
        message("Halt\nORG 100"),
        "Unexpected origination directive."
    );
}

#[test]
fn label_starts_with_digit() {
    assert_eq!(codes("1Foo, Halt"), vec![Code::LABEL_STARTS_WITH_DIGIT]);
    assert_eq!(message("1Foo, Halt"), "Labels cannot start with a number.");
}

#[test]
fn label_contains_whitespace() {
    assert_eq!(
        codes("My Label, Halt"),
        vec![Code::LABEL_CONTAINS_WHITESPACE]
    );
    // A space before the comma lands inside the label rather than being trimmed.
    assert_eq!(codes("Foo , Halt"), vec![Code::LABEL_CONTAINS_WHITESPACE]);
    assert_eq!(
        message("My Label, Halt"),
        "Labels cannot contain whitespace."
    );
}

#[test]
fn duplicate_label() {
    assert_eq!(codes("A, DEC 1\nA, DEC 2"), vec![Code::DUPLICATE_LABEL]);
    let assembly = assemble("A, DEC 1\nA, DEC 2");
    let diagnostic = assembly.errors().next().unwrap();
    // The note points at the first definition so an editor can link the two.
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].span.start, 0);
}

#[test]
fn unknown_label() {
    assert_eq!(codes("Load Nope"), vec![Code::UNKNOWN_LABEL]);
    assert_eq!(message("Load Nope"), "Unknown label 'Nope'.");
}

#[test]
fn unknown_mnemonic() {
    assert_eq!(codes("Frobnicate 1"), vec![Code::UNKNOWN_MNEMONIC]);
    // The message uses the lowercased mnemonic, as MARIE.js does.
    assert_eq!(message("Frobnicate 1"), "Unknown operator 'frobnicate'.");
    // `ORG` and `END` outside their valid forms land here too.
    assert_eq!(codes("ORG 10"), vec![Code::UNKNOWN_MNEMONIC]);
}

#[test]
fn missing_operand() {
    assert_eq!(codes("Load"), vec![Code::MISSING_OPERAND]);
    assert_eq!(message("Load"), "Expected operand for load.");
    // A literal directive uses the shorter message.
    assert_eq!(codes("DEC"), vec![Code::MISSING_OPERAND]);
    assert_eq!(message("DEC"), "Expected operand.");
}

#[test]
fn unexpected_operand() {
    assert_eq!(codes("Halt 1"), vec![Code::UNEXPECTED_OPERAND]);
    assert_eq!(message("Halt 1"), "Unexpected operand '1' for 'halt'.");
}

#[test]
fn malformed_literal() {
    assert_eq!(codes("DEC abc"), vec![Code::MALFORMED_LITERAL]);
    assert_eq!(codes("OCT 8"), vec![Code::MALFORMED_LITERAL]);
    assert_eq!(codes("HEX GG"), vec![Code::MALFORMED_LITERAL]);
    assert_eq!(codes("DEC +1"), vec![Code::MALFORMED_LITERAL]);
    assert_eq!(message("DEC abc"), "Failed to parse operand.");
}

#[test]
fn literal_out_of_range() {
    assert_eq!(codes("DEC 65536"), vec![Code::LITERAL_OUT_OF_RANGE]);
    assert_eq!(codes("DEC -32769"), vec![Code::LITERAL_OUT_OF_RANGE]);
    assert_eq!(codes("HEX 10000"), vec![Code::LITERAL_OUT_OF_RANGE]);
    assert_eq!(codes("OCT 200000"), vec![Code::LITERAL_OUT_OF_RANGE]);
    // An absurdly long literal is out of range rather than a parse failure.
    assert_eq!(
        codes("DEC 999999999999999999999999"),
        vec![Code::LITERAL_OUT_OF_RANGE]
    );
    assert_eq!(message("DEC 65536"), "Literal out of bounds.");
}

#[test]
fn address_out_of_range() {
    assert_eq!(codes("Add 1000"), vec![Code::ADDRESS_OUT_OF_RANGE]);
    assert_eq!(
        codes("Add 99999999999999999999"),
        vec![Code::ADDRESS_OUT_OF_RANGE]
    );
    assert_eq!(message("Add 1000"), "Address 0x1000 is out of bounds.");
}

#[test]
fn program_too_large() {
    // One word more than the address space holds.
    let source = "Halt\n".repeat(mrs_core::MEMORY_WORD_COUNT as usize + 1);
    let assembly = assemble(&source);
    assert_eq!(
        assembly.errors().map(|d| d.code).collect::<Vec<_>>(),
        vec![Code::PROGRAM_TOO_LARGE]
    );
    // Exactly full is fine.
    let exact = "Halt\n".repeat(mrs_core::MEMORY_WORD_COUNT as usize);
    let assembly = assemble(&exact);
    assert!(assembly.succeeded());
    assert!(assembly.image().is_some());
}

#[test]
fn a_program_that_overflows_its_origin_is_rejected() {
    // The single intentional divergence from MARIE.js: it assigns addresses past the
    // end of memory without complaint and only fails when the simulator loads the
    // program, which corrupts the opcode field of any instruction referring to such an
    // address. Reporting it here names the offending line instead. No program that
    // MARIE.js could actually run is rejected by this.
    let source = format!("ORG FFF\n{}", "Halt\n".repeat(2));
    let assembly = assemble(&source);
    assert_eq!(
        assembly.errors().map(|d| d.code).collect::<Vec<_>>(),
        vec![Code::PROGRAM_TOO_LARGE]
    );
    assert!(assembly.image().is_none());

    // Exactly reaching the last word is still fine.
    let exact = format!("ORG FFF\n{}", "Halt\n");
    assert!(assemble(&exact).succeeded());
}

#[test]
fn every_error_is_reported_rather_than_only_the_first() {
    let assembly = assemble("Load Missing\nNope 1\nHalt 2\nDEC 99999\n");
    let found: Vec<_> = assembly.errors().map(|d| d.code).collect();
    assert_eq!(
        found,
        vec![
            Code::UNKNOWN_LABEL,
            Code::UNKNOWN_MNEMONIC,
            Code::UNEXPECTED_OPERAND,
            Code::LITERAL_OUT_OF_RANGE,
        ]
    );
}

#[test]
fn parse_errors_and_assembly_errors_both_appear() {
    let assembly = assemble("Load X Y\nLoad Missing\n");
    assert_eq!(
        assembly.errors().map(|d| d.code).collect::<Vec<_>>(),
        vec![Code::MALFORMED_LINE, Code::UNKNOWN_LABEL]
    );
}

#[test]
fn every_diagnostic_code_has_a_distinct_stable_string() {
    let mut seen: Vec<String> = Code::ALL.iter().map(Code::to_string).collect();
    seen.sort();
    let count = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), count, "diagnostic code strings must be unique");
    // Every built-in code lives in the crate's own namespace.
    assert!(Code::ALL.iter().all(|c| c.is_builtin()));
}

// ---------------------------------------------------------------------------
// Robustness.
// ---------------------------------------------------------------------------

#[test]
fn pathological_input_never_panics() {
    let cases = [
        "",
        "\n",
        "\n\n\n",
        ",",
        ",,,,",
        "/",
        "//",
        "/,",
        " , ",
        "\t\t\t",
        "\r\n\r\n",
        "a,",
        ",a",
        "a,,b",
        "ORG",
        "ORG ",
        "ORG 0",
        "ORG 0000",
        "END",
        "END END",
        "Load",
        "Load ",
        "Load  ",
        "Load X Y Z",
        "\u{e9}",
        "\u{e9}, Halt",
        "Load \u{e9}",
        "\u{1F600}, DEC 1",
        "DEC \u{e9}",
        "----",
        "-1",
        "0x10",
        "Load 0x10",
        "\0",
        "Load \0",
        "A, A, Halt",
        "Halt / / / /",
        "/*not a block comment*/",
        &"Load X\n".repeat(5000),
        &"A".repeat(10_000),
        &format!("{}, Halt", "A".repeat(10_000)),
    ];
    for case in cases {
        let assembly = assemble(case);
        // Whatever happened, the invariants hold.
        assert_eq!(
            assembly.words.len(),
            assembly.program.items.len(),
            "word/item mismatch for {:?}",
            &case[..case.len().min(40)]
        );
        // And linting the same input is equally safe.
        let _ = assemble_with(case, Options::linting());
    }
}

#[test]
fn multi_byte_characters_do_not_corrupt_spans() {
    let source = "caf\u{e9}, Load caf\u{e9}\n";
    let assembly = assemble(source);
    assert!(assembly.succeeded());
    let symbol = assembly.symbols.get("caf\u{e9}").expect("label defined");
    assert_eq!(symbol.definition.text(source), Some("caf\u{e9}"));
    assert_eq!(symbol.references.len(), 1);
    assert_eq!(symbol.references[0].text(source), Some("caf\u{e9}"));
}

#[test]
fn crlf_line_endings_assemble_the_same_as_lf() {
    let lf = "Load X\nHalt\nX, DEC 5\n";
    let crlf = lf.replace('\n', "\r\n");
    let from_lf = assemble(lf);
    let from_crlf = assemble(&crlf);
    assert!(from_crlf.succeeded(), "{:?}", from_crlf.errors().count());
    assert_eq!(from_lf.words, from_crlf.words);
}

#[test]
fn a_file_with_no_trailing_newline_assembles() {
    assert!(assemble("Halt").succeeded());
    assert_eq!(assemble("Halt").words.len(), 1);
}

#[test]
fn every_span_lies_within_the_source() {
    let source = "Foo, Load Bar / c\nBar, DEC 1\nLoad X Y\nDEC 99999\n";
    let assembly = assemble_with(source, Options::linting());
    let limit = source.len() as u32;
    for diagnostic in &assembly.diagnostics {
        assert!(diagnostic.span.end <= limit, "{diagnostic}");
        assert!(diagnostic.span.start <= diagnostic.span.end, "{diagnostic}");
        for related in &diagnostic.labels {
            assert!(related.span.end <= limit);
        }
    }
    for item in &assembly.program.items {
        assert!(item.span.end <= limit);
        assert!(item.span.text(source).is_some(), "span splits a character");
    }
}
