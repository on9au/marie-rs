//! End-to-end: assemble a program, run it on the VM, check the result.
//!
//! These are ported from MARIE.js's `describe('simulator')` block, which assembles
//! source and asserts on the machine state afterwards. They are the strongest
//! compatibility check in the repository, because a divergence anywhere — an operand
//! base, a label address, an opcode encoding, a micro-program — shows up as a wrong
//! number rather than as a passing unit test on a wrong assumption.

use mrs_asm::assemble;
use mrs_vm::{MarieVM, io::VecIo, states::RunOutcome};

/// Assembles and runs `source`, returning the halted machine.
///
/// Panics if the program fails to assemble, faults, or does not halt within a
/// generous step budget.
fn run(
    source: &str,
    inputs: impl IntoIterator<Item = i16>,
) -> MarieVM<VecIo, mrs_vm::states::Terminated> {
    let assembly = assemble(source);
    assert!(
        assembly.succeeded(),
        "assembly failed: {:?}",
        assembly.errors().map(|e| e.to_string()).collect::<Vec<_>>()
    );

    let mut vm = MarieVM::new(VecIo::new(inputs));
    let (origin, words) = assembly.program_image();
    vm.load_program(origin, words).expect("program fits");

    match vm.boot().run_bounded(10_000) {
        RunOutcome::Terminated(vm) => vm,
        RunOutcome::Faulted(_, fault) => panic!("program faulted: {fault}"),
        RunOutcome::Suspended(_, reason) => panic!("program did not halt: {reason}"),
    }
}

/// Runs `source` and returns the accumulator afterwards.
fn accumulator(source: &str) -> i16 {
    run(source, []).registers().ac.value()
}

/// Runs `source` and returns the word at the address of `label`.
fn word_at_label(source: &str, label: &str) -> i16 {
    let assembly = assemble(source);
    let address = assembly
        .symbols
        .address_of(label)
        .unwrap_or_else(|| panic!("no label {label}"));
    run(source, []).memory().read(address)
}

#[test]
fn add_instruction_works() {
    assert_eq!(
        accumulator("\t\tLoad A\n\t\tAdd B\n\t\tHalt\n\t\tA, DEC 1234\n\t\tB, DEC 1010\n\t"),
        2244
    );
}

#[test]
fn subt_instruction_works() {
    assert_eq!(
        accumulator("\t\tLoad A\n\t\tSubt B\n\t\tHalt\n\t\tA, DEC 1234\n\t\tB, DEC 123\n\t"),
        1111
    );
}

#[test]
fn addi_instruction_works() {
    assert_eq!(
        accumulator(
            "\t\tLoad A\n\t\tAddI C\n\t\tHalt\n\t\tA, DEC 1234\n\t\tB, DEC 1010\n\t\tC, DEC 4\n\t"
        ),
        2244
    );
}

#[test]
fn clear_instruction_works() {
    assert_eq!(
        accumulator("\t\tLoad A\n\t\tClear\n\t\tHalt\n\t\tA, DEC 1234\n\t"),
        0
    );
}

#[test]
fn load_instruction_works() {
    assert_eq!(
        accumulator("\t\tLoad A\n\t\tHalt\n\t\tA, DEC 1234\n\t"),
        1234
    );
}

#[test]
fn store_instruction_works() {
    assert_eq!(
        word_at_label(
            "\t\tLoad A\n\t\tStore B\n\t\tHalt\n\t\tA, DEC 1234\n\t\tB, DEC 0\n\t",
            "B"
        ),
        1234
    );
}

#[test]
fn input_instruction_works() {
    let vm = run("\t\tInput\n\t\tHalt\n\t", [1234]);
    assert_eq!(vm.registers().input.value(), 1234);
    assert_eq!(vm.registers().ac.value(), 1234);
}

#[test]
fn output_instruction_works() {
    let vm = run("\t\tLoad A\n\t\tOutput\n\t\tHalt\n\t\tA, DEC 1234\n\t", []);
    assert_eq!(vm.registers().output.value(), 1234);
    assert_eq!(vm.io().outputs, vec![1234]);
}

#[test]
fn jump_instruction_works() {
    assert_eq!(
        accumulator("\t\tJump A\n\t\tHalt\n\t\tA, Load B\n\t\tHalt\n\t\tB, DEC 1234\n\t"),
        1234
    );
}

#[test]
fn jns_instruction_works() {
    // JnS stores the return address, which is 1 here, and does not touch the AC.
    assert_eq!(
        word_at_label("\t\tJns A\n\t\tA, DEC 1234\n\t\tHalt\n\t", "A"),
        1
    );
}

#[test]
fn jumpi_instruction_works() {
    assert_eq!(
        accumulator(
            "\t\tJumpI A\n\t\tHalt\n\t\tLoad B\n\t\tHalt\n\t\tA, DEC 2\n\t\tB, DEC 1234\n\t"
        ),
        1234
    );
}

#[test]
fn loadi_instruction_works() {
    assert_eq!(
        accumulator("\t\tLoadI A\n\t\tHalt\n\t\tA, DEC 3\n\t\tDEC 1234\n\t"),
        1234
    );
}

#[test]
fn storei_instruction_works() {
    assert_eq!(
        word_at_label(
            "\t\tLoad A\n\t\tStoreI B\n\t\tHalt\n\t\tA, DEC 1234\n\t\tB, DEC 5\n\t\tC, DEC 4321\n\t",
            "C"
        ),
        1234
    );
}

#[test]
fn halt_instruction_works() {
    let vm = run("Halt", []);
    assert_eq!(vm.registers().pc.value(), 1);
}

#[test]
fn skipcond_selects_the_right_branch_for_each_condition() {
    // Skips when AC < 0, so the Jump is stepped over and One is loaded.
    let source = "\
        Input
        Skipcond 000
        Jump NoSkip
        Load One
        Halt
NoSkip, Load Zero
        Halt
One,    DEC 1
Zero,   DEC 0
";
    assert_eq!(run(source, [-5]).registers().ac.value(), 1, "AC < 0 skips");
    assert_eq!(
        run(source, [0]).registers().ac.value(),
        0,
        "AC = 0 does not"
    );
    assert_eq!(
        run(source, [5]).registers().ac.value(),
        0,
        "AC > 0 does not"
    );

    let positive = source.replace("Skipcond 000", "Skipcond 800");
    assert_eq!(
        run(&positive, [5]).registers().ac.value(),
        1,
        "AC > 0 skips"
    );

    let zero = source.replace("Skipcond 000", "Skipcond 400");
    assert_eq!(run(&zero, [0]).registers().ac.value(), 1, "AC = 0 skips");

    // The MARIE.js extension: C00 skips when the accumulator is non-zero. It has to
    // be written `0C00`, because an operand starting with `C` is read as a label.
    let non_zero = source.replace("Skipcond 000", "Skipcond 0C00");
    assert_eq!(
        run(&non_zero, [7]).registers().ac.value(),
        1,
        "AC != 0 skips"
    );
    assert_eq!(run(&non_zero, [0]).registers().ac.value(), 0);
}

#[test]
fn org_relocates_a_program_that_still_runs() {
    let vm = run(
        "ORG 200\n        Load X\n        Add X\n        Store Y\n        Halt\nX, DEC 21\nY, DEC 0\n",
        [],
    );
    assert_eq!(vm.registers().ac.value(), 42);
    // Nothing was written below the origin.
    assert_eq!(vm.memory().read(mrs_core::MemoryAddress::new(0)), 0);
}

#[test]
fn a_realistic_program_sums_its_input_until_a_sentinel() {
    let source = "\
        / Sum inputs until a zero is entered, then print the total.
        Clear
        Store Total
Loop,   Input
        Skipcond 400        / stop when the value is zero
        Jump Accumulate
        Load Total
        Output
        Halt
Accumulate, Add Total
        Store Total
        Jump Loop
Total,  DEC 0
";
    let vm = run(source, [5, 10, 27, 0]);
    assert_eq!(vm.io().outputs, vec![42]);
}

#[test]
fn a_subroutine_call_returns_through_jumpi() {
    // JnS/JumpI is the classic MARIE subroutine idiom, and it depends on JnS leaving
    // the accumulator alone.
    let source = "\
        Load Value
        JnS Double
        Store Result
        Halt
Double, HEX 0            / return address lands here
        Add Value
        JumpI Double
Value,  DEC 21
Result, DEC 0
";
    let vm = run(source, []);
    assert_eq!(vm.registers().ac.value(), 42);
    let assembly = assemble(source);
    let result = assembly.symbols.address_of("Result").unwrap();
    assert_eq!(vm.memory().read(result), 42);
}
