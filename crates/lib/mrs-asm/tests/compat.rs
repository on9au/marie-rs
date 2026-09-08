//! Compatibility with the MARIE.js assembler.
//!
//! The cases in the first half are ported from MARIE.js's own `src/marie.test.ts`, so
//! that a divergence shows up as a failure here rather than as a program that quietly
//! computes something else. The second half covers rules that are only visible in
//! MARIE.js's source — the operand base, the literal-versus-label rule, the strict
//! shape of `ORG` — which its test suite does not exercise but which any real program
//! depends on.
//!
//! MARIE.js stores program words as unsigned 16-bit integers; these compare against
//! `u16` for that reason, so the expected values can be copied across unchanged.

use mrs_asm::{Code, assemble};

/// Assembles, asserting success, and returns the words as unsigned patterns.
fn words(source: &str) -> Vec<u16> {
    let assembly = assemble(source);
    assert!(
        assembly.succeeded(),
        "expected success, got: {:?}",
        assembly.errors().map(|e| e.to_string()).collect::<Vec<_>>()
    );
    assembly.words.iter().map(|w| *w as u16).collect()
}

/// Returns the error codes produced by a source file, in order.
fn error_codes(source: &str) -> Vec<Code> {
    assemble(source).errors().map(|e| e.code).collect()
}

// ---------------------------------------------------------------------------
// Ported directly from MARIE.js's `describe('assembler')`.
// ---------------------------------------------------------------------------

#[test]
fn can_assemble_all_instructions() {
    let assembled = words(
        "
 \t\tLabel, DEC 1
 \t\tHEX 2
 \t\tOCT 3
 \t\tAdd Label
 \t\tSubt Label
 \t\tAddI Label
 \t\tClear
 \t\tLoad Label
 \t\tStore Label
 \t\tInput
 \t\tOutput
 \t\tJump Label
 \t\tSkipcond 800
 \t\tJnS Label
 \t\tLoadI Label
 \t\tStoreI Label
 \t\tHalt
\t",
    );
    assert_eq!(
        assembled,
        vec![
            0x0001, 0x0002, 0x0003, 0x3000, 0x4000, 0xb000, 0xa000, 0x1000, 0x2000, 0x5000, 0x6000,
            0x9000, 0x8800, 0x0000, 0xd000, 0xe000, 0x7000,
        ]
    );
}

#[test]
fn rejects_duplicate_labels() {
    let assembly = assemble("\n \t\tLabel, Add Label\n\t\tLabel, Halt\n\t");
    let errors: Vec<_> = assembly.errors().collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, Code::DUPLICATE_LABEL);
    assert_eq!(
        errors[0].message,
        "Labels must be unique. The label 'Label' was already defined on line 2."
    );
    // The note points back at the first definition, for an editor to link.
    assert_eq!(errors[0].labels.len(), 1);
}

#[test]
fn rejects_unknown_labels() {
    let assembly = assemble("\n \t\tAdd Foo\n\t");
    let errors: Vec<_> = assembly.errors().collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, Code::UNKNOWN_LABEL);
    assert_eq!(errors[0].message, "Unknown label 'Foo'.");
}

#[test]
fn data_accepts_valid_values() {
    let assembled = words(
        "
 \t\tDEC -100
\t\tDEC 0
\t\tDEC 100
\t\tHEX FFFF
\t\tHEX 0A
\t\tOCT 7
\t\tOCT 177777
\t\tOCT 004
\t",
    );
    assert_eq!(
        assembled,
        vec![
            0xff9c, 0x0000, 0x0064, 0xffff, 0x000a, 0x0007, 0xffff, 0x0004
        ]
    );
}

#[test]
fn data_rejects_values_out_of_range() {
    assert!(assemble("DEC 65535").succeeded());
    assert!(!assemble("DEC 65536").succeeded());
    assert!(assemble("DEC -32768").succeeded());
    assert!(!assemble("DEC -32769").succeeded());
    assert!(!assemble("HEX 10000").succeeded());
    assert!(!assemble("OCT 200000").succeeded());

    assert_eq!(error_codes("DEC 65536"), vec![Code::LITERAL_OUT_OF_RANGE]);
    assert_eq!(error_codes("HEX 10000"), vec![Code::LITERAL_OUT_OF_RANGE]);
}

#[test]
fn rejects_address_literals_out_of_range() {
    assert!(assemble("Add 123").succeeded());
    assert!(assemble("Add 0FFF").succeeded());
    assert!(!assemble("Add 1000").succeeded());

    let assembly = assemble("Add 1000");
    let errors: Vec<_> = assembly.errors().collect();
    assert_eq!(errors[0].code, Code::ADDRESS_OUT_OF_RANGE);
    assert_eq!(errors[0].message, "Address 0x1000 is out of bounds.");
}

// ---------------------------------------------------------------------------
// Rules taken from MARIE.js's source rather than its tests.
// ---------------------------------------------------------------------------

#[test]
fn instruction_operands_are_hexadecimal() {
    // The single most surprising rule: `Add 123` is address 0x123, not decimal 123.
    assert_eq!(words("Add 123"), vec![0x3123]);
    assert_eq!(words("Add 0FF"), vec![0x30FF]);
    assert_eq!(words("Skipcond 400"), vec![0x8400]);
    // ...even though a DEC literal on the next line is decimal.
    assert_eq!(words("Add 010\nDEC 010"), vec![0x3010, 10]);
}

#[test]
fn an_operand_starting_with_a_digit_is_a_literal_and_anything_else_is_a_label() {
    // `1A` starts with a digit, so it is the address 0x1A.
    assert_eq!(words("Load 1A\nHalt"), vec![0x101A, 0x7000]);
    // `A1` does not, so it is a label — and an undefined one here.
    assert_eq!(error_codes("Load A1"), vec![Code::UNKNOWN_LABEL]);
    // Defined, it resolves to its address rather than to 0xA1.
    assert_eq!(words("Load A1\nA1, DEC 7"), vec![0x1001, 0x0007]);
}

#[test]
fn skipcond_c00_must_be_written_with_a_leading_zero() {
    // A trap worth its own test. The non-zero condition is 0xC00, but `C00` does not
    // start with a *decimal* digit, so MARIE.js's /^\d[0-9a-fA-F]*$/ reads it as a
    // label reference and the program fails to assemble. Writing it as `0C00` makes it
    // a literal again. This is why the MARIE.js documentation spells this one condition
    // `0C00` while the other three are plain `000`, `400` and `800`.
    assert_eq!(error_codes("Skipcond C00"), vec![Code::UNKNOWN_LABEL]);
    assert_eq!(words("Skipcond 0C00"), vec![0x8C00]);
    // The same trap catches any operand beginning with a hex letter.
    assert_eq!(error_codes("Load FFF"), vec![Code::UNKNOWN_LABEL]);
    assert_eq!(words("Load 0FFF"), vec![0x1FFF]);
}

#[test]
fn labels_are_case_sensitive_but_mnemonics_are_not() {
    // Three spellings of one instruction.
    assert_eq!(words("halt"), vec![0x7000]);
    assert_eq!(words("HALT"), vec![0x7000]);
    assert_eq!(words("HaLt"), vec![0x7000]);
    // Two distinct labels, which must not collide.
    let assembly = assemble("Foo, DEC 1\nfoo, DEC 2\nLoad Foo\nLoad foo");
    assert!(
        assembly.succeeded(),
        "labels differing only in case collided"
    );
    assert_eq!(assembly.words[2] as u16, 0x1000);
    assert_eq!(assembly.words[3] as u16, 0x1001);
}

#[test]
fn org_sets_the_origin_and_shifts_every_label() {
    let assembly = assemble("ORG 100\nLoad X\nHalt\nX, DEC 9");
    assert!(assembly.succeeded());
    assert_eq!(assembly.origin.value(), 0x100);
    // X is the third word, so 0x102.
    assert_eq!(assembly.symbols.address_of("X").unwrap().value(), 0x102);
    assert_eq!(assembly.words[0] as u16, 0x1102);
}

#[test]
fn org_is_only_the_strict_three_hex_digit_form() {
    assert_eq!(assemble("ORG 100\nHalt").origin.value(), 0x100);
    assert_eq!(assemble("org 0ff\nHalt").origin.value(), 0x0ff);
    assert_eq!(assemble("OrG A0B\nHalt").origin.value(), 0xa0b);

    // Anything else is parsed as a statement, and `org` is not an instruction.
    for source in ["ORG 10", "ORG 1000", "ORG xyz", "ORG100"] {
        assert_eq!(
            error_codes(source),
            vec![Code::UNKNOWN_MNEMONIC],
            "for {source:?}"
        );
    }
}

#[test]
fn org_must_come_first_and_only_once() {
    assert_eq!(
        error_codes("Halt\nORG 100"),
        vec![Code::UNEXPECTED_ORIGIN],
        "an origin after a word"
    );
    assert_eq!(
        error_codes("ORG 100\nORG 200\nHalt"),
        vec![Code::UNEXPECTED_ORIGIN],
        "a second origin"
    );
    // The rejected directive does not change the origin that was already set.
    assert_eq!(assemble("ORG 100\nORG 200\nHalt").origin.value(), 0x100);
}

#[test]
fn end_stops_assembly_and_hides_everything_after_it() {
    let assembly = assemble("Load X\nEND\nX, DEC 5\nthis is not even valid");
    // `X` was never defined, because the line defining it is past END.
    assert_eq!(
        assembly.errors().map(|e| e.code).collect::<Vec<_>>(),
        vec![Code::UNKNOWN_LABEL]
    );
    // The garbage line produces no diagnostic at all.
    assert_eq!(assembly.errors().count(), 1);
    assert_eq!(assembly.words.len(), 1);
}

#[test]
fn a_label_on_the_end_line_is_still_defined() {
    // MARIE.js records the label before it notices the END, so it points one word past
    // the program.
    let assembly = assemble("Halt\nAfter, END");
    assert_eq!(assembly.symbols.address_of("After").unwrap().value(), 1);
}

#[test]
fn adr_emits_a_bare_address_and_clear_is_load_immi_zero() {
    // ADR assembles as JnS, whose opcode is zero, so the word is just the address.
    assert_eq!(words("ADR X\nX, DEC 7"), vec![0x0001, 0x0007]);
    assert_eq!(words("ADR 123"), vec![0x0123]);
    assert_eq!(words("Clear"), vec![0xA000]);
    // MARIE.js overwrites any operand written on a Clear, rather than rejecting it.
    assert_eq!(words("Clear 5"), vec![0xA000]);
}

#[test]
fn operandless_instructions_reject_an_operand() {
    for mnemonic in ["Input", "Output", "Halt"] {
        assert_eq!(words(mnemonic).len(), 1);
        assert_eq!(
            error_codes(&format!("{mnemonic} 1")),
            vec![Code::UNEXPECTED_OPERAND],
            "for {mnemonic}"
        );
    }
}

#[test]
fn instructions_that_need_an_operand_must_get_one() {
    for mnemonic in ["Load", "Store", "Add", "Subt", "Jump", "JnS", "AddI"] {
        assert_eq!(
            error_codes(mnemonic),
            vec![Code::MISSING_OPERAND],
            "for {mnemonic}"
        );
    }
    // A literal directive with no operand gives the shorter message MARIE.js uses.
    let assembly = assemble("DEC");
    assert_eq!(
        assembly.errors().next().unwrap().message,
        "Expected operand."
    );
}

#[test]
fn only_decimal_literals_may_be_signed() {
    assert_eq!(words("DEC -1"), vec![0xFFFF]);
    // MARIE.js matches decimal with /^-?\d+$/, so a leading plus is a parse failure.
    assert_eq!(error_codes("DEC +1"), vec![Code::MALFORMED_LITERAL]);
    // The other bases have no sign in their pattern at all.
    assert_eq!(error_codes("HEX -1"), vec![Code::MALFORMED_LITERAL]);
    assert_eq!(error_codes("OCT -1"), vec![Code::MALFORMED_LITERAL]);
}

#[test]
fn comments_run_from_a_slash_to_the_end_of_the_line() {
    assert_eq!(words("Halt / stop here"), vec![0x7000]);
    assert_eq!(words("/ whole line\nHalt"), vec![0x7000]);
    assert_eq!(words("Load X/c\nX, DEC 1"), vec![0x1001, 0x0001]);
    // A comment-only line emits nothing, so addresses do not shift.
    assert_eq!(words("/ a\n/ b\nHalt"), vec![0x7000]);
}

#[test]
fn an_invalid_label_skips_the_whole_line_and_shifts_addresses() {
    // MARIE.js reports the bad label and moves on without emitting a word, so `Halt`
    // lands at address 0 rather than 1 — and `X` with it.
    let assembly = assemble("1Bad, DEC 5\nX, Halt");
    assert_eq!(
        assembly.errors().map(|e| e.code).collect::<Vec<_>>(),
        vec![Code::LABEL_STARTS_WITH_DIGIT]
    );
    assert_eq!(assembly.symbols.address_of("X").unwrap().value(), 0);
    assert_eq!(assembly.words, vec![0x7000_u16 as i16]);
}

#[test]
fn a_zero_operand_and_leading_zeros_are_in_range() {
    assert_eq!(words("Add 0"), vec![0x3000]);
    assert_eq!(words("Add 000"), vec![0x3000]);
    assert_eq!(words("Add 00000FFF"), vec![0x3FFF]);
    // Still out of range once the zeros are stripped.
    assert_eq!(error_codes("Add 0001000"), vec![Code::ADDRESS_OUT_OF_RANGE]);
}

#[test]
fn every_opcode_and_directive_assembles_from_its_mnemonic() {
    use mrs_core::{Directive, Opcode};

    for opcode in Opcode::ALL {
        let source = if opcode.takes_operand() {
            format!("{} 001", opcode.mnemonic())
        } else {
            opcode.mnemonic().to_owned()
        };
        let assembled = words(&source);
        assert_eq!(
            assembled[0] >> 12,
            u16::from(opcode.to_nibble()),
            "for {opcode}"
        );
    }

    // Every directive is recognised in its own valid form.
    for directive in Directive::ALL {
        let source = match directive {
            Directive::Org => "ORG 100\nHalt".to_owned(),
            Directive::End => "END".to_owned(),
            Directive::Clear => "Clear".to_owned(),
            Directive::Adr => "ADR 001".to_owned(),
            other => format!("{} 1", other.mnemonic()),
        };
        assert!(assemble(&source).succeeded(), "for {directive}");
    }
}
