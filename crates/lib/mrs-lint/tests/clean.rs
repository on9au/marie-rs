//! Correct programs must produce nothing.
//!
//! This is the suite that decides whether the linter is usable. A tool that flags
//! working code gets switched off and then catches nothing at all, so every program
//! here is one that assembles, runs on the VM, and produces the right answer — and is
//! asserted to draw no findings whatsoever.

use mrs_lint::Linter;
use mrs_vm::{MarieVM, io::VecIo, states::RunOutcome};

/// Lints `source`, asserting it is completely clean, then runs it.
///
/// Returns the halted machine so the caller can check the program was correct as well
/// as tidy — a program the linter likes but that computes garbage proves nothing.
fn clean_and_correct(
    source: &str,
    inputs: impl IntoIterator<Item = i16>,
) -> MarieVM<VecIo, mrs_vm::states::Terminated> {
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
    assert!(
        outcome.diagnostics.is_empty(),
        "expected no findings, got:\n{}",
        outcome
            .diagnostics
            .iter()
            .map(|d| format!("  {d}"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let mut vm = MarieVM::new(VecIo::new(inputs));
    let (origin, words) = outcome.assembly.program_image();
    vm.load_program(origin, words).unwrap();
    match vm.boot().run_bounded(10_000) {
        RunOutcome::Terminated(vm) => vm,
        RunOutcome::Faulted(_, fault) => panic!("faulted: {fault}"),
        RunOutcome::Suspended(_, reason) => panic!("did not halt: {reason}"),
    }
}

#[test]
fn a_straight_line_program() {
    let vm = clean_and_correct(
        "\
/ Add two numbers.
        Load  First
        Add   Second
        Store Total
        Halt
First,  DEC 21
Second, DEC 21
Total,  DEC 0
",
        [],
    );
    assert_eq!(vm.registers().ac.value(), 42);
}

#[test]
fn a_counted_loop() {
    let vm = clean_and_correct(
        "\
/ Sum 1..5.
        Clear
        Store Total
        Load  Count
Loop,   Store Count
        Load  Total
        Add   Count
        Store Total
        Load  Count
        Subt  One
        Skipcond 800
        Jump  Done
        Jump  Loop
Done,   Load  Total
        Output
        Halt
Count,  DEC 5
Total,  DEC 0
One,    DEC 1
",
        [],
    );
    assert_eq!(vm.io().outputs, vec![15]);
}

#[test]
fn an_input_loop_with_a_sentinel() {
    let vm = clean_and_correct(
        "\
/ Sum inputs until a zero is entered.
        Clear
        Store Total
Loop,   Input
        Skipcond 400
        Jump  Accumulate
        Load  Total
        Output
        Halt
Accumulate, Add Total
        Store Total
        Jump  Loop
Total,  DEC 0
",
        [5, 10, 27, 0],
    );
    assert_eq!(vm.io().outputs, vec![42]);
}

#[test]
fn a_subroutine_called_through_jns_and_returning_through_jumpi() {
    let vm = clean_and_correct(
        "\
/ Double a value in a subroutine.
        Load  Value
        JnS   Double
        Store Result
        Halt
Double, HEX 0
        Add   Value
        JumpI Double
Value,  DEC 21
Result, DEC 0
",
        [],
    );
    assert_eq!(vm.registers().ac.value(), 42);
}

#[test]
fn a_program_using_every_addressing_mode() {
    let vm = clean_and_correct(
        "\
        LoadI Pointer
        AddI  Pointer
        StoreI Sink
        Halt
Pointer, ADR Value
Sink,   ADR Result
Value,  DEC 21
Result, DEC 0
",
        [],
    );
    assert_eq!(vm.registers().ac.value(), 42);
}

#[test]
fn a_relocated_program() {
    let vm = clean_and_correct(
        "\
ORG 100
        Load  X
        Add   X
        Output
        Halt
X,      DEC 21
",
        [],
    );
    assert_eq!(vm.io().outputs, vec![42]);
    assert_eq!(vm.registers().pc.value(), 0x104);
}

#[test]
fn a_program_ending_in_end() {
    let vm = clean_and_correct(
        "\
        Load  X
        Output
        Halt
X,      DEC 7
        END
",
        [],
    );
    assert_eq!(vm.io().outputs, vec![7]);
}

#[test]
fn all_four_skip_conditions_in_one_program() {
    let vm = clean_and_correct(
        "\
        Load  Zero
        Skipcond 400
        Jump  Bad
        Load  Neg
        Skipcond 000
        Jump  Bad
        Load  Pos
        Skipcond 800
        Jump  Bad
        Load  Pos
        Skipcond 0C00
        Jump  Bad
        Load  Good
        Output
        Halt
Bad,    Load  Zero
        Output
        Halt
Good,   DEC 1
Zero,   DEC 0
Neg,    DEC -1
Pos,    DEC 1
",
        [],
    );
    assert_eq!(vm.io().outputs, vec![1], "every condition behaved");
}

#[test]
fn an_infinite_loop_that_never_reaches_data_is_not_flagged_for_flow() {
    // Deliberately non-terminating, but control never escapes into the variables, so
    // the flow lints have nothing to say. Only `missing-halt` should fire.
    let source = "Spin,   Jump  Spin\n";
    let outcome = Linter::new().check(source);
    let codes: Vec<_> = outcome.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        vec![mrs_lint::lints::style::MISSING_HALT],
        "{:#?}",
        outcome.diagnostics
    );
}
