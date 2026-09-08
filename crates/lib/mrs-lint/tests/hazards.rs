//! Lints for code that assembles cleanly and then does something else.

use mrs_lint::lints::hazards::{
    JNS_OVERWRITES_CODE, MASKED_SKIPCOND, SELF_MODIFYING_CODE, SKIPCOND_LABEL_OPERAND,
};
use mrs_lint::{Linter, Outcome};
use mrs_vm::{MarieVM, io::VecIo, states::RunOutcome};

fn check(source: &str) -> Outcome {
    let outcome = Linter::new().check(source);
    assert!(
        outcome.assembled(),
        "should assemble: {:?}",
        outcome
            .assembly
            .errors()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
    outcome
}

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

/// Branches one way or the other depending on how `Skipcond` reads its operand.
fn skipcond_probe(operand: &str) -> String {
    format!(
        "\
        Load  One
        Skipcond {operand}
        Jump  NoSkip
        Jump  Skipped
NoSkip, Load  Two
        Halt
Skipped, Load Three
        Halt
One,    DEC 1
Two,    DEC 2
Three,  DEC 3
"
    )
}

#[test]
fn a_masked_skipcond_is_reported_and_silently_tests_the_wrong_thing() {
    let broken = skipcond_probe("100");
    let outcome = check(&broken);
    assert!(outcome.has(MASKED_SKIPCOND), "{:#?}", outcome.diagnostics);

    // The finding says it behaves as `Skipcond 000`. Prove it on the machine: with a
    // positive accumulator, 800 skips and 100 does not.
    assert_eq!(run_ac(&skipcond_probe("800")), Some(3), "AC > 0 skips");
    assert_eq!(
        run_ac(&broken),
        Some(2),
        "100 is not a fifth condition: it tests AC < 0 and does not skip"
    );
    // Which is exactly what writing 000 does.
    assert_eq!(run_ac(&skipcond_probe("000")), Some(2));
}

#[test]
fn the_four_real_conditions_are_not_reported() {
    for operand in ["000", "400", "800", "0C00"] {
        let outcome = check(&skipcond_probe(operand));
        assert!(
            !outcome.has(MASKED_SKIPCOND),
            "Skipcond {operand} should be accepted: {:#?}",
            outcome.diagnostics
        );
    }
}

#[test]
fn the_finding_names_the_condition_actually_tested() {
    let outcome = check(&skipcond_probe("401"));
    let message = &outcome
        .diagnostics
        .iter()
        .find(|d| d.code == MASKED_SKIPCOND)
        .unwrap()
        .message;
    assert!(message.contains("Skipcond 400"), "{message}");
    assert!(message.contains("AC = 0"), "{message}");
}

#[test]
fn a_skipcond_taking_a_label_is_reported() {
    // `Skipcond Done` reads as a condition selected by Done's address, not a branch.
    let source = "\
        Load  One
        Skipcond Done
        Halt
Done,   Halt
One,    DEC 1
";
    let outcome = check(source);
    assert!(
        outcome.has(SKIPCOND_LABEL_OPERAND),
        "{:#?}",
        outcome.diagnostics
    );
    let diagnostic = outcome
        .diagnostics
        .iter()
        .find(|d| d.code == SKIPCOND_LABEL_OPERAND)
        .unwrap();
    // It points at the label's definition as a secondary note.
    assert_eq!(diagnostic.labels.len(), 1);
}

#[test]
fn a_jns_onto_code_is_reported_and_really_overwrites_it() {
    let source = "\
        JnS   Target
        Halt
Target, Output
        Halt
";
    let outcome = check(source);
    assert!(
        outcome.has(JNS_OVERWRITES_CODE),
        "{:#?}",
        outcome.diagnostics
    );

    // The word at Target held `Output` (0x6000) and is replaced by the return address.
    let assembly = mrs_asm::assemble(source);
    let target = assembly.symbols.address_of("Target").unwrap();
    assert_eq!(assembly.words[target.index()] as u16, 0x6000);

    let mut vm = MarieVM::new(VecIo::new([]));
    let (origin, words) = assembly.program_image();
    vm.load_program(origin, words).unwrap();
    let RunOutcome::Terminated(vm) = vm.boot().run_bounded(5_000) else {
        panic!("should halt");
    };
    assert_eq!(
        vm.memory().read(target),
        1,
        "the instruction was replaced by the return address"
    );
}

#[test]
fn a_jns_onto_a_reserved_word_is_not_reported() {
    // The correct idiom: the return slot is data.
    let source = "\
        JnS   Sub
        Halt
Sub,    HEX 0
        Output
        JumpI Sub
";
    let outcome = check(source);
    assert!(
        !outcome.has(JNS_OVERWRITES_CODE),
        "{:#?}",
        outcome.diagnostics
    );
}

#[test]
fn a_store_over_a_live_instruction_is_advice() {
    let source = "\
        Load  Patch
        Store Victim
Victim, Output
        Halt
Patch,  HEX 7000
";
    let outcome = check(source);
    assert!(
        outcome.has(SELF_MODIFYING_CODE),
        "{:#?}",
        outcome.diagnostics
    );
    let diagnostic = outcome
        .diagnostics
        .iter()
        .find(|d| d.code == SELF_MODIFYING_CODE)
        .unwrap();
    // Deliberate self-modification is legal, so this is advice, not a warning.
    assert_eq!(diagnostic.severity, mrs_asm::Severity::Advice);
}

#[test]
fn an_ordinary_store_to_a_variable_is_not_advice() {
    let source = "        Load  X\n        Store Y\n        Halt\nX, DEC 1\nY, DEC 0\n";
    let outcome = check(source);
    assert!(
        !outcome.has(SELF_MODIFYING_CODE),
        "{:#?}",
        outcome.diagnostics
    );
}
