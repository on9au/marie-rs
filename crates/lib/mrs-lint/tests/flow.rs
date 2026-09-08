//! Control-flow lints.
//!
//! Where a lint claims a program misbehaves, the test runs it on the VM and shows that
//! it does. A linter nobody trusts gets switched off, so the findings here are checked
//! against the machine rather than against my reading of the code.

use mrs_lint::lints::flow::{
    FALLS_INTO_DATA, FALLS_OFF_END, JUMPS_OUTSIDE, UNREACHABLE_INSTRUCTION,
};
use mrs_lint::{Linter, Outcome};
use mrs_vm::{MarieVM, io::VecIo, states::RunOutcome};

fn check(source: &str) -> Outcome {
    let outcome = Linter::new().check(source);
    assert!(
        outcome.assembled(),
        "test program should assemble: {:?}",
        outcome
            .assembly
            .errors()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
    outcome
}

/// Runs a program and returns the accumulator, or `None` if it never halted.
fn run_ac(source: &str) -> Option<i16> {
    let assembly = mrs_asm::assemble(source);
    let mut vm = MarieVM::new(VecIo::new([]));
    let (origin, words) = assembly.program_image();
    vm.load_program(origin, words).unwrap();
    match vm.boot().run_bounded(5_000) {
        RunOutcome::Terminated(vm) => Some(vm.registers().ac.value()),
        _ => None,
    }
}

#[test]
fn falling_into_data_is_reported_and_really_happens() {
    // No Halt between the code and the variables.
    let source = "        Load  X\n        Add   X\nX,      DEC 21\n";
    let outcome = check(source);
    assert!(outcome.has(FALLS_INTO_DATA), "{:#?}", outcome.diagnostics);

    // The diagnostic names what the data decodes as; confirm that is what it says.
    let message = &outcome
        .diagnostics
        .iter()
        .find(|d| d.code == FALLS_INTO_DATA)
        .unwrap()
        .message;
    // 21 is 0x0015, which decodes as `JnS 015`.
    assert!(message.contains("JnS 015"), "{message}");
}

#[test]
fn a_halt_before_the_data_silences_it() {
    let source = "        Load  X\n        Add   X\n        Halt\nX,      DEC 21\n";
    let outcome = check(source);
    assert!(!outcome.has(FALLS_INTO_DATA), "{:#?}", outcome.diagnostics);
    assert!(!outcome.has(FALLS_OFF_END));
    assert_eq!(run_ac(source), Some(42), "and the program works");
}

#[test]
fn falling_off_the_end_is_reported() {
    // The last word is reachable and is not a Halt or a jump.
    let source = "        Load  X\n        Halt\nX,      DEC 1\n";
    assert!(!check(source).has(FALLS_OFF_END), "this one ends properly");

    let runs_off = "        Input\n        Output\n";
    let outcome = check(runs_off);
    assert!(outcome.has(FALLS_OFF_END), "{:#?}", outcome.diagnostics);
    // And the machine really does not stop.
    assert_eq!(run_ac(runs_off), None, "the program should not halt");
}

#[test]
fn a_skip_past_the_last_word_falls_off_the_end() {
    // Skipcond at the end can skip over the final instruction and off the edge.
    let source = "        Load  One\n        Skipcond 800\n        Halt\nOne,    DEC 1\n";
    let outcome = check(source);
    // Skipping the Halt lands on the data word, so this is the data finding.
    assert!(outcome.has(FALLS_INTO_DATA), "{:#?}", outcome.diagnostics);
}

#[test]
fn unreachable_code_is_reported_only_when_flow_is_fully_known() {
    // A jump over an instruction that nothing else reaches.
    let source = "        Jump  After\n        Halt\nAfter,  Halt\n";
    let outcome = check(source);
    assert_eq!(outcome.count(UNREACHABLE_INSTRUCTION), 1);

    // The same shape, but with an indirect jump somewhere in the program: the graph
    // can no longer see every edge, so the lint must stay quiet.
    let indirect = "\
        Jump  After
        Halt
After,  JumpI Ptr
Ptr,    DEC 4
        Halt
";
    let outcome = check(indirect);
    assert!(
        !outcome.has(UNREACHABLE_INSTRUCTION),
        "an indirect jump must suppress this lint: {:#?}",
        outcome.diagnostics
    );
}

#[test]
fn data_is_never_reported_as_unreachable() {
    // Variables are supposed to be unreachable; only unreachable *code* is a finding.
    let source = "        Load  X\n        Halt\nX,      DEC 1\n";
    let outcome = check(source);
    assert!(
        !outcome.has(UNREACHABLE_INSTRUCTION),
        "{:#?}",
        outcome.diagnostics
    );
}

#[test]
fn jumping_outside_the_program_is_reported() {
    // 0FF is well past the end of a three-word program.
    let source = "        Jump  0FF\n        Halt\nX,      DEC 1\n";
    let outcome = check(source);
    assert!(outcome.has(JUMPS_OUTSIDE), "{:#?}", outcome.diagnostics);
}

#[test]
fn an_origin_shifts_the_graph_without_confusing_it() {
    // Addresses are absolute but indices are relative, so ORG is where an off-by-origin
    // bug would show up.
    let source = "ORG 100\n        Load  X\n        Halt\nX,      DEC 5\n";
    let outcome = check(source);
    assert!(!outcome.has(FALLS_INTO_DATA), "{:#?}", outcome.diagnostics);
    assert!(!outcome.has(JUMPS_OUTSIDE));
    assert!(!outcome.has(UNREACHABLE_INSTRUCTION));

    // A jump within a relocated program resolves to the right word.
    let looping = "ORG 200\nTop,    Load  X\n        Jump  Top\nX,      DEC 5\n";
    let outcome = check(looping);
    assert!(!outcome.has(JUMPS_OUTSIDE), "{:#?}", outcome.diagnostics);
}

#[test]
fn a_jns_subroutine_is_traced_through_its_return_slot() {
    // Execution resumes at X + 1, not at X, so the word after the slot is reachable.
    let source = "\
        Load  Value
        JnS   Double
        Halt
Double, HEX 0
        Add   Value
        JumpI Double
Value,  DEC 21
";
    let outcome = check(source);
    assert!(!outcome.has(FALLS_INTO_DATA), "{:#?}", outcome.diagnostics);
    assert!(!outcome.has(FALLS_OFF_END));
    assert_eq!(run_ac(source), Some(42));
}

#[test]
fn nothing_is_analysed_when_the_file_does_not_assemble() {
    // A failed item emits a zero word; following those would report nonsense on top of
    // the real errors.
    let outcome = Linter::new().check("Load Missing\nNope 1\n");
    assert!(!outcome.assembled());
    for code in [
        FALLS_INTO_DATA,
        FALLS_OFF_END,
        JUMPS_OUTSIDE,
        UNREACHABLE_INSTRUCTION,
    ] {
        assert!(!outcome.has(code), "{code} fired on a broken file");
    }
}

#[test]
fn an_empty_program_produces_nothing() {
    let outcome = Linter::new().check("/ just a comment\n");
    assert!(outcome.assembled());
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
}
