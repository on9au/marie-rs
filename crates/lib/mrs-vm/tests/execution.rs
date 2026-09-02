//! End-to-end tests for the fetch-decode-execute engine.
//!
//! These pin down the MARIE.js semantics, including the places where MARIE.js
//! deliberately differs from the textbook MARIE.

use std::task::Poll;

use mrs_vm::{
    MarieVM,
    instruction::{Instruction, Opcode},
    io::{Flaky, IoError, MarieVmIODevice, VecIo},
    memory::MemoryAddress,
    microcode::MicroOp,
    states::{Fault, RunOutcome, StepOutcome, SuspendReason, Terminated},
    value::Value,
};

fn addr(address: u16) -> MemoryAddress {
    MemoryAddress::new(address)
}

/// Assembles `(opcode, operand)` pairs into machine words.
fn asm(program: &[(Opcode, u16)]) -> Vec<i16> {
    program
        .iter()
        .map(|(opcode, operand)| Instruction::new(*opcode, addr(*operand)).encode().value())
        .collect()
}

/// Runs a program from address 0 and returns the halted VM.
fn run(program: &[i16], inputs: &[i16]) -> MarieVM<VecIo, Terminated> {
    let mut vm = MarieVM::new(VecIo::new(inputs.iter().copied()));
    vm.load_program(addr(0), program).unwrap();
    match vm.boot().run() {
        RunOutcome::Terminated(vm) => vm,
        RunOutcome::Faulted(_, fault) => panic!("unexpected fault: {fault}"),
        RunOutcome::Suspended(vm, reason) => {
            panic!("unexpected suspend at {}: {reason}", vm.registers().pc)
        }
    }
}

#[test]
fn load_add_store_output_halt() {
    use Opcode::*;
    let mut program = asm(&[
        (Load, 0x005),
        (Add, 0x006),
        (Store, 0x007),
        (Output, 0x000),
        (Halt, 0x000),
    ]);
    program.extend_from_slice(&[40, 2, 0]);

    let vm = run(&program, &[]);
    assert_eq!(vm.registers().ac, Value::new(42));
    assert_eq!(vm.memory().read(addr(0x007)), 42);
    assert_eq!(vm.io().outputs, vec![42]);
    // Output mirrors the accumulator into the OUT register.
    assert_eq!(vm.registers().output, Value::new(42));
}

#[test]
fn subt_and_arithmetic_wrap_at_16_bits() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x004), (Subt, 0x005), (Output, 0), (Halt, 0)]);
    program.extend_from_slice(&[i16::MIN, 1]);

    let vm = run(&program, &[]);
    assert_eq!(vm.io().outputs, vec![i16::MAX]);
}

#[test]
fn load_immi_loads_a_zero_extended_12_bit_immediate() {
    use Opcode::*;
    // 0xAFFF must load 4095, not -1: the immediate is unsigned.
    let program = asm(&[(LoadImmi, 0xFFF), (Output, 0), (Halt, 0)]);
    let vm = run(&program, &[]);
    assert_eq!(vm.io().outputs, vec![4095]);

    // `Clear` is just `LoadImmi 0`.
    let program = asm(&[(LoadImmi, 0xFFF), (LoadImmi, 0x000), (Output, 0), (Halt, 0)]);
    let vm = run(&program, &[]);
    assert_eq!(vm.io().outputs, vec![0]);
}

#[test]
fn input_writes_the_in_register_then_the_accumulator() {
    use Opcode::*;
    let program = asm(&[(Input, 0), (Output, 0), (Halt, 0)]);
    let vm = run(&program, &[-7]);
    assert_eq!(vm.registers().input, Value::new(-7));
    assert_eq!(vm.registers().ac, Value::new(-7));
    assert_eq!(vm.io().outputs, vec![-7]);
}

#[test]
fn skipcond_covers_all_four_conditions() {
    use Opcode::*;
    // A skip advances the PC past address 0x002, so the two arms are the `Jump`s at
    // 0x002 (condition false) and 0x003 (condition true).
    let cases: [(u16, i16, bool); 8] = [
        (0x000, -1, true), // AC < 0
        (0x000, 0, false),
        (0x400, 0, true), // AC == 0
        (0x400, 1, false),
        (0x800, 1, true), // AC > 0
        (0x800, -1, false),
        (0xC00, -1, true), // AC != 0
        (0xC00, 0, false),
    ];

    for (condition, ac, expected_skip) in cases {
        let mut program = asm(&[
            (Load, 0x009),         // 000: AC = the value under test
            (SkipCond, condition), // 001
            (Jump, 0x004),         // 002: reached when the condition does NOT hold
            (Jump, 0x006),         // 003: reached when it does (002 was skipped)
            (Output, 0),           // 004: not-skipped path echoes the AC
            (Halt, 0),             // 005
            (LoadImmi, 0x099),     // 006: skipped path outputs a sentinel
            (Output, 0),           // 007
            (Halt, 0),             // 008
        ]);
        program.push(ac); // 009

        let vm = run(&program, &[]);
        let outputs = &vm.io().outputs;
        if expected_skip {
            assert_eq!(
                outputs,
                &vec![0x099],
                "cond {condition:03X} ac {ac} should skip"
            );
        } else {
            assert_eq!(
                outputs,
                &vec![ac],
                "cond {condition:03X} ac {ac} should not skip"
            );
        }
    }
}

#[test]
fn skipcond_ignores_the_low_bits_of_its_operand() {
    use Opcode::*;
    // 0x4FF selects the same condition as 0x400.
    let program = asm(&[
        (LoadImmi, 0),
        (SkipCond, 0x4FF),
        (Halt, 0),
        (LoadImmi, 7),
        (Halt, 0),
    ]);
    let vm = run(&program, &[]);
    assert_eq!(vm.registers().ac, Value::new(7));
}

#[test]
fn jns_stores_the_return_address_and_preserves_the_accumulator() {
    use Opcode::*;
    let mut program = asm(&[
        (LoadImmi, 0x007), // 000: AC = 7
        (JnS, 0x005),      // 001: M[005] = 002, PC = 006
        (Output, 0),       // 002
        (Halt, 0),         // 003
        (Halt, 0),         // 004: padding, never reached
        (Halt, 0),         // 005: overwritten with the return address
        (Add, 0x00A),      // 006: AC += 5
        (JumpI, 0x005),    // 007: return
        (Halt, 0),         // 008
        (Halt, 0),         // 009
    ]);
    program.push(5); // 00A

    let vm = run(&program, &[]);
    // The return address stored at 005 is the instruction after the JnS.
    assert_eq!(vm.memory().read(addr(0x005)), 0x002);
    // 7 + 5: the textbook JnS would have clobbered AC with 0x006 and produced 11.
    assert_eq!(vm.io().outputs, vec![12]);
}

#[test]
fn indirect_load_store_and_add() {
    use Opcode::*;
    let mut program = asm(&[
        (LoadI, 0x006),    // 000: AC = M[M[006]] = M[00A] = 42
        (Store, 0x00B),    // 001
        (LoadImmi, 0x001), // 002: AC = 1
        (StoreI, 0x007),   // 003: M[M[007]] = M[00C] = 1
        (AddI, 0x006),     // 004: AC = 1 + M[M[006]] = 43
        (Halt, 0),         // 005
    ]);
    program.extend_from_slice(&[
        0x00A, // 006: pointer to 00A
        0x00C, // 007: pointer to 00C
        0, 0,  // 008, 009
        42, // 00A
        0,  // 00B
        0,  // 00C
    ]);

    let vm = run(&program, &[]);
    assert_eq!(vm.registers().ac, Value::new(43));
    assert_eq!(vm.memory().read(addr(0x00B)), 42);
    assert_eq!(vm.memory().read(addr(0x00C)), 1);
}

#[test]
fn jumpi_discards_the_high_nibble_of_the_pointer() {
    use Opcode::*;
    let mut program = asm(&[(JumpI, 0x003), (Halt, 0), (Halt, 0)]);
    // MARIE has 12 address lines, so 0x9004 is a jump to 0x004, not a fault.
    program.push(Value::from_bits(0x9004).value());
    program.extend_from_slice(&asm(&[(LoadImmi, 0x123), (Halt, 0)]));

    let vm = run(&program, &[]);
    assert_eq!(vm.registers().ac, Value::new(0x123));
}

#[test]
fn program_counter_wraps_at_the_top_of_memory() {
    use Opcode::*;
    let mut vm = MarieVM::new(VecIo::new([]));
    vm.load_program(MemoryAddress::MAX, &asm(&[(LoadImmi, 0x005)]))
        .unwrap();
    vm.memory_mut().write(addr(0), asm(&[(Halt, 0)])[0]);

    let RunOutcome::Terminated(vm) = vm.boot().run() else {
        panic!("expected halt");
    };
    assert_eq!(vm.registers().ac, Value::new(5));
}

#[test]
fn sums_inputs_until_a_zero_sentinel() {
    use Opcode::*;
    let mut program = asm(&[
        (Input, 0),        // 000
        (SkipCond, 0x400), // 001: skip if AC == 0
        (Jump, 0x004),     // 002
        (Jump, 0x009),     // 003
        (Store, 0x00D),    // 004: Temp = AC
        (Load, 0x00C),     // 005: AC = Sum
        (Add, 0x00D),      // 006
        (Store, 0x00C),    // 007: Sum = AC
        (Jump, 0x000),     // 008
        (Load, 0x00C),     // 009
        (Output, 0),       // 00A
        (Halt, 0),         // 00B
    ]);
    program.extend_from_slice(&[0, 0]); // 00C Sum, 00D Temp

    let vm = run(&program, &[3, 4, 5, 0]);
    assert_eq!(vm.io().outputs, vec![12]);
}

#[test]
fn unassigned_opcode_faults_with_the_offending_address() {
    let mut vm = MarieVM::new(VecIo::new([]));
    vm.load_program(addr(0), &[asm(&[(Opcode::Jump, 0x004)])[0]])
        .unwrap();
    vm.memory_mut()
        .write(addr(4), Value::from_bits(0xF123).value());

    let RunOutcome::Faulted(vm, fault) = vm.boot().run() else {
        panic!("expected a fault");
    };
    let Fault::InvalidOpcode { address, word } = fault else {
        panic!("expected InvalidOpcode, got {fault:?}");
    };
    assert_eq!(address, addr(4));
    assert_eq!(word, Value::from_bits(0xF123));
    // The word that faulted is still visible in IR.
    assert_eq!(vm.registers().ir, Value::from_bits(0xF123));
}

#[test]
fn input_at_end_of_stream_faults() {
    use Opcode::*;
    let mut vm = MarieVM::new(VecIo::new([]));
    vm.load_program(addr(0), &asm(&[(Input, 0), (Halt, 0)]))
        .unwrap();

    let RunOutcome::Faulted(_, fault) = vm.boot().run() else {
        panic!("expected a fault");
    };
    assert!(
        matches!(
            fault,
            Fault::Io { address, opcode: Opcode::Input, error: IoError::Eof }
                if address == addr(0)
        ),
        "got {fault:?}"
    );
}

#[test]
fn output_failure_faults() {
    struct BrokenOutput;
    impl MarieVmIODevice for BrokenOutput {
        fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
            Poll::Ready(Err(IoError::Eof))
        }
        fn output(&mut self, _value: i16) -> Result<(), IoError> {
            Err(IoError::Parse("broken".into()))
        }
    }

    use Opcode::*;
    let mut vm = MarieVM::new(BrokenOutput);
    vm.load_program(addr(0), &asm(&[(LoadImmi, 1), (Output, 0), (Halt, 0)]))
        .unwrap();

    let RunOutcome::Faulted(_, fault) = vm.boot().run() else {
        panic!("expected a fault");
    };
    assert_eq!(fault.address(), addr(1));
    assert!(matches!(
        fault,
        Fault::Io {
            opcode: Opcode::Output,
            ..
        }
    ));
}

#[test]
fn pending_input_suspends_without_consuming_the_instruction() {
    use Opcode::*;
    let mut vm = MarieVM::new(Flaky::new(VecIo::new([9]), 1));
    vm.load_program(addr(0), &asm(&[(Input, 0), (Output, 0), (Halt, 0)]))
        .unwrap();

    let RunOutcome::Suspended(vm, reason) = vm.boot().run() else {
        panic!("expected a suspend");
    };
    assert_eq!(reason, SuspendReason::AwaitingInput);
    // The machine is stalled inside the Input instruction, at the operation that
    // reads the device, so resuming re-polls it.
    assert!(!vm.at_instruction_boundary());
    assert_eq!(vm.instruction_address(), addr(0));
    assert_eq!(vm.next_micro_op(), Some(MicroOp::ReadInput));
    assert_eq!(vm.registers().ac, Value::ZERO);

    let RunOutcome::Terminated(vm) = vm.run() else {
        panic!("expected the resumed program to halt");
    };
    assert_eq!(vm.into_io().into_inner().outputs, vec![9]);
}

#[test]
fn stepping_advances_one_instruction_at_a_time() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x003), (Output, 0), (Halt, 0)]);
    program.push(11);

    let mut vm = MarieVM::new(VecIo::new([]));
    vm.load_program(addr(0), &program).unwrap();
    let vm = vm.debug();

    assert_eq!(vm.next_instruction().unwrap().opcode(), Load);

    // Load: MAR ends up at the operand, MBR holds the fetched datum, PC has advanced.
    let StepOutcome::Stepped(vm) = vm.step() else {
        panic!("expected a step");
    };
    assert_eq!(vm.registers().pc, addr(1));
    assert_eq!(vm.registers().mar, addr(3));
    assert_eq!(vm.registers().mbr, Value::new(11));
    assert_eq!(vm.registers().ac, Value::new(11));
    assert_eq!(vm.registers().ir, Instruction::new(Load, addr(3)).encode());
    assert!(vm.io().outputs.is_empty());

    let StepOutcome::Stepped(vm) = vm.step() else {
        panic!("expected a step");
    };
    assert_eq!(vm.io().outputs, vec![11]);

    let StepOutcome::Terminated(vm) = vm.step() else {
        panic!("expected termination");
    };
    // Halt leaves the PC past the Halt instruction.
    assert_eq!(vm.registers().pc, addr(3));
}

#[test]
fn stepping_reports_pending_input_without_advancing() {
    use Opcode::*;
    let mut vm = MarieVM::new(Flaky::new(VecIo::new([4]), 2));
    vm.load_program(addr(0), &asm(&[(Input, 0), (Halt, 0)]))
        .unwrap();

    let mut vm = vm.debug();
    for _ in 0..2 {
        let StepOutcome::AwaitingInput(next) = vm.step() else {
            panic!("expected to be waiting for input");
        };
        assert_eq!(next.next_micro_op(), Some(MicroOp::ReadInput));
        assert_eq!(next.instruction_address(), addr(0));
        vm = next;
    }

    let StepOutcome::Stepped(vm) = vm.step() else {
        panic!("expected the input to land");
    };
    assert_eq!(vm.registers().ac, Value::new(4));
}

#[test]
fn breakpoints_suspend_before_executing_and_can_be_resumed() {
    use Opcode::*;
    // A three-iteration countdown so the breakpoint at 0x001 is hit repeatedly.
    let mut program = asm(&[
        (Load, 0x006),     // 000
        (Subt, 0x007),     // 001  <- breakpoint
        (Store, 0x006),    // 002
        (SkipCond, 0x400), // 003
        (Jump, 0x000),     // 004
        (Halt, 0),         // 005
    ]);
    program.extend_from_slice(&[3, 1]); // 006 counter, 007 one

    let mut vm = MarieVM::new(VecIo::new([]));
    vm.load_program(addr(0), &program).unwrap();
    vm.breakpoints_mut().insert(addr(0x001));
    assert!(vm.breakpoints().contains(addr(0x001)));

    let mut running = vm.boot();
    let mut hits = 0;
    let terminated = loop {
        match running.run() {
            RunOutcome::Suspended(vm, SuspendReason::Breakpoint(address)) => {
                assert_eq!(address, addr(0x001));
                // The breakpointed instruction has not run yet: AC is the counter.
                assert_eq!(vm.registers().ac, Value::new(3 - hits));
                hits += 1;
                running = vm;
            }
            RunOutcome::Terminated(vm) => break vm,
            other => panic!(
                "unexpected outcome: {}",
                match other {
                    RunOutcome::Faulted(_, fault) => fault.to_string(),
                    RunOutcome::Suspended(_, reason) => reason.to_string(),
                    RunOutcome::Terminated(_) => unreachable!(),
                }
            ),
        }
    };
    assert_eq!(hits, 3);
    assert_eq!(terminated.memory().read(addr(0x006)), 0);
}

#[test]
fn run_bounded_stops_at_the_step_limit_and_can_continue() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x004), (Add, 0x004), (Output, 0), (Halt, 0)]);
    program.push(6);

    let mut vm = MarieVM::new(VecIo::new([]));
    vm.load_program(addr(0), &program).unwrap();

    let RunOutcome::Suspended(vm, reason) = vm.boot().run_bounded(2) else {
        panic!("expected the budget to run out");
    };
    assert_eq!(reason, SuspendReason::StepLimit);
    assert_eq!(vm.registers().pc, addr(2));
    assert_eq!(vm.registers().ac, Value::new(12));
    assert!(vm.io().outputs.is_empty());

    let RunOutcome::Terminated(vm) = vm.run() else {
        panic!("expected the program to halt");
    };
    assert_eq!(vm.io().outputs, vec![12]);
}

#[test]
fn run_bounded_terminates_an_infinite_loop() {
    use Opcode::*;
    let mut vm = MarieVM::new(VecIo::new([]));
    vm.load_program(addr(0), &asm(&[(Jump, 0x000)])).unwrap();

    let RunOutcome::Suspended(_, reason) = vm.boot().run_bounded(1_000) else {
        panic!("expected the budget to run out");
    };
    assert_eq!(reason, SuspendReason::StepLimit);
}

#[test]
fn reset_clears_the_machine_but_keeps_the_device_and_breakpoints() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x003), (Output, 0), (Halt, 0)]);
    program.push(5);

    let mut vm = MarieVM::new(VecIo::new([]));
    vm.load_program(addr(0), &program).unwrap();
    vm.breakpoints_mut().insert(addr(0x002));

    let RunOutcome::Suspended(vm, _) = vm.boot().run() else {
        panic!("expected the breakpoint to hit");
    };
    let RunOutcome::Terminated(vm) = vm.run() else {
        panic!("expected the program to halt");
    };

    let vm = vm.reset();
    assert_eq!(vm.registers().pc, MemoryAddress::ZERO);
    assert_eq!(vm.registers().ac, Value::ZERO);
    assert_eq!(vm.memory().read(addr(0x003)), 0);
    // The device and its recorded output survive the reset.
    assert_eq!(vm.io().outputs, vec![5]);
    assert!(vm.breakpoints().contains(addr(0x002)));
}

#[test]
fn every_opcode_is_reachable_from_a_decoded_word() {
    // Guards against an opcode being added to the enum but not to the executor.
    for opcode in Opcode::ALL {
        let word = Instruction::new(opcode, addr(0x001)).encode();
        assert_eq!(
            Instruction::decode(word).map(Instruction::opcode),
            Some(opcode)
        );
    }
}

#[test]
fn executing_arbitrary_memory_never_panics() {
    // Every address the engine computes is masked to 12 bits, so no memory image can
    // drive it out of bounds, overflow an address, or trip an `expect`. Sweep random
    // images to hold that property honest.
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        // xorshift64*; good enough to shake out structure, and reproducible.
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    for _ in 0..64 {
        let words: Vec<i16> = (0..4096).map(|_| next() as i16).collect();
        // A few inputs so `Input` sometimes succeeds rather than always faulting.
        let inputs: Vec<i16> = (0..8).map(|_| next() as i16).collect();

        let mut vm = MarieVM::new(VecIo::new(inputs));
        vm.load_program(addr(0), &words).unwrap();
        vm.registers_mut().pc = MemoryAddress::new_masked(next() as u16);

        // Whatever happens, it must be one of the modelled outcomes.
        match vm.boot().run_bounded(10_000) {
            RunOutcome::Terminated(_) | RunOutcome::Faulted(..) => {}
            RunOutcome::Suspended(_, reason) => {
                assert_eq!(reason, SuspendReason::StepLimit);
            }
        }
    }
}
