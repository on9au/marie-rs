//! Tests for micro-stepping and stepping backwards.

use std::collections::VecDeque;
use std::task::Poll;

use mrs_vm::{
    MarieVM,
    history::StepBackError,
    instruction::{Instruction, Opcode},
    io::{IoError, MarieVmIODevice, VecIo},
    memory::MemoryAddress,
    microcode::{self, MicroOp},
    registers::{Register, Registers},
    states::{MicroStepOutcome, StepOutcome, Terminated},
    value::Value,
};

fn addr(address: u16) -> MemoryAddress {
    MemoryAddress::new(address)
}

fn asm(program: &[(Opcode, u16)]) -> Vec<i16> {
    program
        .iter()
        .map(|(opcode, operand)| Instruction::new(*opcode, addr(*operand)).encode().value())
        .collect()
}

/// A program that exercises arithmetic, memory, control flow and both I/O directions.
fn mixed_program() -> Vec<i16> {
    use Opcode::*;
    let mut program = asm(&[
        (Input, 0),        // 000: read a value
        (Store, 0x00B),    // 001
        (LoadImmi, 0x003), // 002
        (Add, 0x00B),      // 003
        (Output, 0),       // 004
        (Store, 0x00C),    // 005
        (SkipCond, 0x800), // 006: skip if positive
        (Jump, 0x009),     // 007
        (LoadI, 0x00D),    // 008: indirect load
        (Halt, 0),         // 009
    ]);
    program.extend_from_slice(&[
        0,     // 00A: padding
        0,     // 00B
        0,     // 00C
        0x00A, // 00D: pointer
    ]);
    program
}

/// Everything about a machine that stepping backwards must restore.
#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    registers: Registers,
    memory: Vec<i16>,
    micro_pc: u8,
    decoded: Option<Instruction>,
    instruction_address: MemoryAddress,
    io: VecIo,
}

fn snapshot<S>(vm: &MarieVM<VecIo, S>) -> Snapshot {
    Snapshot {
        registers: *vm.registers(),
        memory: vm.memory().as_slice().to_vec(),
        micro_pc: vm.micro_pc(),
        decoded: vm.decoded_instruction(),
        instruction_address: vm.instruction_address(),
        io: vm.io().clone(),
    }
}

fn debuggable(
    program: &[i16],
    inputs: &[i16],
    history: usize,
) -> MarieVM<VecIo, mrs_vm::states::Stepping> {
    let mut vm = MarieVM::new(VecIo::new(inputs.iter().copied()));
    vm.load_program(addr(0), program).unwrap();
    vm.set_history_limit(history);
    vm.debug()
}

#[test]
fn micro_stepping_walks_the_documented_register_transfers() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x002), (Halt, 0)]);
    program.push(77);

    let mut vm = debuggable(&program, &[], 0);
    let mut ops = Vec::new();
    for _ in 0..microcode::cycle_length(Load) {
        let MicroStepOutcome::Stepped(next, op) = vm.micro_step() else {
            panic!("expected a micro-step");
        };
        ops.push(op);
        vm = next;
    }

    assert_eq!(
        ops,
        vec![
            MicroOp::Transfer {
                target: Register::Mar,
                source: Register::Pc
            },
            MicroOp::ReadMemory,
            MicroOp::Transfer {
                target: Register::Ir,
                source: Register::Mbr
            },
            MicroOp::IncrementPc,
            MicroOp::Decode,
            MicroOp::Transfer {
                target: Register::Mar,
                source: Register::Ir
            },
            MicroOp::ReadMemory,
            MicroOp::Transfer {
                target: Register::Ac,
                source: Register::Mbr
            },
        ]
    );
    // A whole instruction has run, so we are back at a boundary.
    assert!(vm.at_instruction_boundary());
    assert_eq!(vm.registers().ac, Value::new(77));
    assert_eq!(vm.instruction_address(), addr(0));
}

#[test]
fn micro_steps_are_visible_part_way_through_an_instruction() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x002), (Halt, 0)]);
    program.push(77);

    // Three micro-steps in: fetched and latched into IR, PC not yet incremented.
    let mut vm = debuggable(&program, &[], 0);
    for _ in 0..3 {
        let MicroStepOutcome::Stepped(next, _) = vm.micro_step() else {
            panic!("expected a micro-step");
        };
        vm = next;
    }
    assert!(!vm.at_instruction_boundary());
    assert_eq!(vm.micro_pc(), 3);
    assert_eq!(vm.registers().ir, Instruction::new(Load, addr(2)).encode());
    assert_eq!(vm.registers().pc, addr(0), "PC increments on the next step");
    // Not decoded yet, so the AC has not been touched.
    assert_eq!(vm.decoded_instruction(), None);
    assert_eq!(vm.registers().ac, Value::ZERO);
    assert_eq!(vm.next_micro_op(), Some(MicroOp::IncrementPc));
}

#[test]
fn step_finishes_an_instruction_that_micro_stepping_started() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x002), (Halt, 0)]);
    program.push(77);

    let mut vm = debuggable(&program, &[], 0);
    for _ in 0..2 {
        let MicroStepOutcome::Stepped(next, _) = vm.micro_step() else {
            panic!("expected a micro-step");
        };
        vm = next;
    }

    let StepOutcome::Stepped(vm) = vm.step() else {
        panic!("expected a step");
    };
    assert!(vm.at_instruction_boundary());
    assert_eq!(vm.registers().ac, Value::new(77));
    assert_eq!(vm.registers().pc, addr(1));
}

#[test]
fn micro_stepping_and_stepping_agree_on_the_final_state() {
    let program = mixed_program();

    let mut stepped = debuggable(&program, &[9], 0);
    let by_instruction: MarieVM<VecIo, Terminated> = loop {
        match stepped.step() {
            StepOutcome::Stepped(next) => stepped = next,
            StepOutcome::Terminated(vm) => break vm,
            StepOutcome::AwaitingInput(_) => panic!("device is always ready"),
            StepOutcome::Faulted(_, fault) => panic!("unexpected fault: {fault}"),
        }
    };

    let mut micro_stepped = debuggable(&program, &[9], 0);
    let by_micro_op: MarieVM<VecIo, Terminated> = loop {
        match micro_stepped.micro_step() {
            MicroStepOutcome::Stepped(next, _) => micro_stepped = next,
            MicroStepOutcome::Terminated(vm) => break vm,
            MicroStepOutcome::AwaitingInput(_) => panic!("device is always ready"),
            MicroStepOutcome::Faulted(_, fault) => panic!("unexpected fault: {fault}"),
        }
    };

    assert_eq!(snapshot(&by_instruction), snapshot(&by_micro_op));
    assert_eq!(by_instruction.io().outputs, vec![12]);
}

#[test]
fn stepping_back_restores_every_intermediate_state_exactly() {
    let program = mixed_program();
    let mut vm = debuggable(&program, &[9], 4096);

    // Record the state before every micro-operation on the way forward.
    let mut forward = Vec::new();
    let terminated = loop {
        forward.push(snapshot(&vm));
        match vm.micro_step() {
            MicroStepOutcome::Stepped(next, _) => vm = next,
            MicroStepOutcome::Terminated(t) => break t,
            MicroStepOutcome::AwaitingInput(_) => panic!("device is always ready"),
            MicroStepOutcome::Faulted(_, fault) => panic!("unexpected fault: {fault}"),
        }
    };
    assert!(
        forward.len() > 40,
        "program should be long enough to be interesting"
    );

    // `Halt` changes nothing, so the stopped machine matches the state before it.
    let mut vm = terminated.debug();
    assert_eq!(snapshot(&vm), *forward.last().unwrap());

    // Now walk all the way back, checking each state on the way.
    for expected in forward.iter().rev().skip(1) {
        vm.micro_step_back()
            .expect("history should reach the start");
        assert_eq!(&snapshot(&vm), expected);
    }

    // The journal is exhausted at the very beginning.
    assert_eq!(vm.micro_step_back(), Err(StepBackError::NoHistory));
    assert!(vm.history().is_empty());
    assert!(vm.at_instruction_boundary());
    assert_eq!(vm.registers(), &Registers::new());
    assert_eq!(vm.io().outputs, Vec::<i16>::new());
    assert_eq!(vm.io().inputs, VecDeque::from([9]));
}

#[test]
fn rewinding_completely_and_replaying_reproduces_the_run() {
    let program = mixed_program();
    let mut vm = debuggable(&program, &[9], 4096);

    let start = snapshot(&vm);
    let first_run = loop {
        match vm.step() {
            StepOutcome::Stepped(next) => vm = next,
            StepOutcome::Terminated(t) => break t,
            StepOutcome::AwaitingInput(_) => panic!("device is always ready"),
            StepOutcome::Faulted(_, fault) => panic!("unexpected fault: {fault}"),
        }
    };
    let finished = snapshot(&first_run);

    // Wind the whole program back instruction by instruction. The stopped machine is
    // part-way through the `Halt`, so the first call unwinds that fetch and the rest
    // take one instruction each.
    let mut vm = first_run.debug();
    while !vm.history().is_empty() {
        vm.step_back().expect("history should reach the start");
    }
    assert_eq!(
        snapshot(&vm),
        start,
        "rewinding must restore the initial state"
    );

    // Replaying reaches the same place, including the recovered input.
    let second_run = loop {
        match vm.step() {
            StepOutcome::Stepped(next) => vm = next,
            StepOutcome::Terminated(t) => break t,
            StepOutcome::AwaitingInput(_) => panic!("device is always ready"),
            StepOutcome::Faulted(_, fault) => panic!("unexpected fault: {fault}"),
        }
    };
    assert_eq!(snapshot(&second_run), finished);
}

#[test]
fn step_back_undoes_exactly_one_instruction() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x003), (Add, 0x003), (Halt, 0)]);
    program.push(4);

    let mut vm = debuggable(&program, &[], 1024);
    let StepOutcome::Stepped(vm2) = vm.step() else {
        panic!("expected a step");
    };
    let after_load = snapshot(&vm2);
    let StepOutcome::Stepped(mut vm3) = vm2.step() else {
        panic!("expected a step");
    };
    assert_eq!(vm3.registers().ac, Value::new(8));

    let undone = vm3.step_back().unwrap();
    assert_eq!(undone, microcode::cycle_length(Add));
    assert_eq!(snapshot(&vm3), after_load);
    assert_eq!(vm3.registers().ac, Value::new(4));

    // A second one takes us back to the very beginning.
    vm = vm3;
    vm.step_back().unwrap();
    assert_eq!(vm.registers().ac, Value::ZERO);
    assert_eq!(vm.registers().pc, addr(0));
}

#[test]
fn step_back_from_a_boundary_rewinds_to_the_previous_instruction() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x003), (Add, 0x003), (Halt, 0)]);
    program.push(4);

    // Part-way through the second instruction, `step_back` returns to its start
    // rather than skipping over the first.
    let mut vm = debuggable(&program, &[], 1024);
    let StepOutcome::Stepped(next) = vm.step() else {
        panic!("expected a step");
    };
    vm = next;
    for _ in 0..3 {
        let MicroStepOutcome::Stepped(next, _) = vm.micro_step() else {
            panic!("expected a micro-step");
        };
        vm = next;
    }
    assert!(!vm.at_instruction_boundary());

    assert_eq!(vm.step_back().unwrap(), 3);
    assert!(vm.at_instruction_boundary());
    assert_eq!(vm.registers().pc, addr(1), "still about to run the Add");
    assert_eq!(vm.registers().ac, Value::new(4));
}

#[test]
fn step_back_restores_overwritten_memory() {
    use Opcode::*;
    let mut program = asm(&[(LoadImmi, 0x007), (Store, 0x003), (Halt, 0)]);
    program.push(-1); // 003: about to be overwritten

    let mut vm = debuggable(&program, &[], 1024);
    for _ in 0..2 {
        let StepOutcome::Stepped(next) = vm.step() else {
            panic!("expected a step");
        };
        vm = next;
    }
    assert_eq!(vm.memory().read(addr(3)), 7);

    vm.step_back().unwrap();
    assert_eq!(vm.memory().read(addr(3)), -1);
}

#[test]
fn step_back_rewinds_a_rewindable_device() {
    use Opcode::*;
    let program = asm(&[(Input, 0), (Output, 0), (Halt, 0)]);
    let mut vm = debuggable(&program, &[5], 1024);

    for _ in 0..2 {
        let StepOutcome::Stepped(next) = vm.step() else {
            panic!("expected a step");
        };
        vm = next;
    }
    assert_eq!(vm.io().outputs, vec![5]);
    assert!(vm.io().inputs.is_empty());

    // Undo the Output: the written value is retracted.
    vm.step_back().unwrap();
    assert_eq!(vm.io().outputs, Vec::<i16>::new());

    // Undo the Input: the consumed value goes back on the queue.
    vm.step_back().unwrap();
    assert_eq!(vm.io().inputs, VecDeque::from([5]));
    assert_eq!(vm.registers().ac, Value::ZERO);
}

/// A device that cannot rewind, which is the default for the trait.
#[derive(Default)]
struct OneWayIo {
    inputs: VecDeque<i16>,
    outputs: Vec<i16>,
}

impl MarieVmIODevice for OneWayIo {
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        Poll::Ready(self.inputs.pop_front().ok_or(IoError::Eof))
    }

    fn output(&mut self, value: i16) -> Result<(), IoError> {
        self.outputs.push(value);
        Ok(())
    }
}

#[test]
fn stepping_back_over_irreversible_io_reports_it_and_changes_nothing() {
    use Opcode::*;
    let program = asm(&[(Input, 0), (Output, 0), (Halt, 0)]);

    let mut vm = MarieVM::new(OneWayIo {
        inputs: VecDeque::from([5]),
        outputs: Vec::new(),
    });
    vm.load_program(addr(0), &program).unwrap();
    vm.set_history_limit(1024);
    let mut vm = vm.debug();

    for _ in 0..2 {
        let StepOutcome::Stepped(next) = vm.step() else {
            panic!("expected a step");
        };
        vm = next;
    }

    let registers = *vm.registers();
    let recorded = vm.history().len();
    let outputs = vm.io().outputs.clone();

    // A single micro-step back is all-or-nothing: the device refuses, and the machine
    // is left exactly as it was.
    assert_eq!(vm.micro_step_back(), Err(StepBackError::IrreversibleOutput));
    assert_eq!(vm.registers(), &registers, "a refused undo changes nothing");
    assert_eq!(vm.history().len(), recorded, "and consumes no history");
    assert_eq!(vm.io().outputs, outputs);

    // Here the refused operation is the most recent one, so `step_back` also gets no
    // further and the machine stays put.
    assert_eq!(vm.step_back(), Err(StepBackError::IrreversibleOutput));
    assert!(vm.at_instruction_boundary());
    assert_eq!(vm.history().len(), recorded);
    assert_eq!(vm.io().outputs, outputs, "the output was never retracted");

    // For an `Input`, the reversible half of the instruction is undone before the
    // refusal, which is the partial rewind `step_back` documents.
    let mut vm = MarieVM::new(OneWayIo {
        inputs: VecDeque::from([5]),
        outputs: Vec::new(),
    });
    vm.load_program(addr(0), &asm(&[(Input, 0), (Halt, 0)]))
        .unwrap();
    vm.set_history_limit(1024);
    let vm = vm.debug();
    let StepOutcome::Stepped(mut vm) = vm.step() else {
        panic!("expected a step");
    };
    assert_eq!(vm.step_back(), Err(StepBackError::IrreversibleInput));
    // `AC <- IN` was reversed and the machine now sits at that operation; the device
    // then refused to give the consumed value back, so the rewind stopped there.
    assert!(!vm.at_instruction_boundary());
    assert_eq!(
        vm.next_micro_op(),
        Some(MicroOp::Transfer {
            target: Register::Ac,
            source: Register::In
        })
    );
    assert_eq!(
        vm.registers().ac,
        Value::ZERO,
        "the AC transfer is reversible"
    );
    assert_eq!(
        vm.registers().input,
        Value::new(5),
        "the consumed value is not"
    );

    // The machine is left consistent rather than merely un-torn: the value is still
    // latched in IN, so stepping on completes the instruction without re-reading the
    // device, and the accumulator comes back.
    let StepOutcome::Stepped(vm) = vm.step() else {
        panic!("expected the instruction to finish");
    };
    assert!(vm.at_instruction_boundary());
    assert_eq!(vm.registers().ac, Value::new(5));
}

#[test]
fn history_is_off_until_a_limit_is_set() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x002), (Halt, 0)]);
    program.push(1);

    let vm = debuggable(&program, &[], 0);
    assert!(!vm.history().is_enabled());
    let StepOutcome::Stepped(mut vm) = vm.step() else {
        panic!("expected a step");
    };
    assert!(vm.history().is_empty());
    assert_eq!(vm.step_back(), Err(StepBackError::NoHistory));
    assert_eq!(vm.micro_step_back(), Err(StepBackError::NoHistory));
}

#[test]
fn a_short_history_limits_how_far_back_you_can_go() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x004), (Add, 0x004), (Add, 0x004), (Halt, 0)]);
    program.push(1);

    // Room for three micro-operations only.
    let mut vm = debuggable(&program, &[], 3);
    for _ in 0..3 {
        let StepOutcome::Stepped(next) = vm.step() else {
            panic!("expected a step");
        };
        vm = next;
    }
    assert_eq!(vm.history().len(), 3);

    // Three undos are available; the fourth has been evicted.
    for _ in 0..3 {
        vm.micro_step_back().unwrap();
    }
    assert_eq!(vm.micro_step_back(), Err(StepBackError::NoHistory));
}

#[test]
fn stepping_back_from_a_halt_re_runs_it() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x002), (Halt, 0)]);
    program.push(3);

    let mut vm = debuggable(&program, &[], 1024);
    let terminated = loop {
        match vm.step() {
            StepOutcome::Stepped(next) => vm = next,
            StepOutcome::Terminated(t) => break t,
            other => panic!("unexpected outcome: {}", outcome_name(&other)),
        }
    };

    // Return to the debugger, undo the Halt instruction, and run it again.
    let mut vm = terminated.debug();
    vm.step_back().unwrap();
    assert_eq!(vm.registers().pc, addr(1), "back to the Halt instruction");
    assert!(matches!(vm.step(), StepOutcome::Terminated(_)));
}

#[test]
fn loading_a_program_discards_stale_history() {
    use Opcode::*;
    let mut program = asm(&[(Load, 0x002), (Halt, 0)]);
    program.push(3);

    let vm = debuggable(&program, &[], 1024);
    let StepOutcome::Stepped(vm) = vm.step() else {
        panic!("expected a step");
    };
    assert!(!vm.history().is_empty());

    // Loading a program discards a journal that describes a machine which no longer
    // exists, but keeps the configured limit.
    let mut ready = MarieVM::new(VecIo::new([]));
    ready.set_history_limit(1024);
    ready.load_program(addr(0), &program).unwrap();
    assert!(ready.history().is_empty());
    assert!(ready.history().is_enabled(), "the limit itself is kept");
}

fn outcome_name<IO>(outcome: &StepOutcome<IO>) -> &'static str {
    match outcome {
        StepOutcome::Stepped(_) => "stepped",
        StepOutcome::AwaitingInput(_) => "awaiting input",
        StepOutcome::Terminated(_) => "terminated",
        StepOutcome::Faulted(..) => "faulted",
    }
}
