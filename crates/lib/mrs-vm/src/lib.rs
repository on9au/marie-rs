//! The MARIE Virtual Machine (VM) crate
//!
//! Contains the main [`MarieVM`] and modules for the ALU, I/O, memory, and
//! registers. The instruction encoding and the address space itself live in
//! [`mrs_core`], so an assembler or linter can share them without depending on the
//! VM; the relevant types are re-exported here.
//!
//! The implemented instruction set is the one used by [MARIE.js], which differs from
//! the textbook (Null & Lobur) MARIE in three ways worth knowing about:
//!
//! - opcode `0xA` is `LoadImmi` (`AC <- IR & 0xFFF`), not `Clear`; `Clear` is an
//!   assembler alias for `LoadImmi 0`,
//! - `Skipcond C00` is defined, and skips if `AC != 0`,
//! - `JnS` does not clobber the accumulator.
//!
//! # Type states
//!
//! The VM is parameterised by a [state](states::MarieVmState) marker that determines
//! which operations are available:
//!
//! ```text
//!                 boot                pause
//!   ┌── Ready ───────────► Running ◄──────────► Stepping ──┐
//!   │     ▲   └── debug ──────┘ │     resume        ▲      │
//!   │     │                     │                   │      │
//!   │     │  reset              ▼      debug        │      │
//!   │     └──────────────── Terminated ─────────────┘      │
//!   └───────────────────────────▲─────────────────────────-┘
//!            halt / fault, from Running or Stepping
//! ```
//!
//! # Execution granularity
//!
//! [`MarieVM::run`] executes until the program stops. [`MarieVM::step`] executes one
//! instruction. [`MarieVM::micro_step`] executes one register-transfer-level
//! [`MicroOp`](microcode::MicroOp), which is how MARIE.js drives its animation and
//! how a debugger can show the fetch-decode-execute cycle in progress.
//!
//! With a [history limit](MarieVM::set_history_limit) set, [`MarieVM::step_back`] and
//! [`MarieVM::micro_step_back`] reverse execution.
//!
//! # Example
//!
//! ```
//! use mrs_vm::{
//!     MarieVM,
//!     instruction::{Instruction, Opcode},
//!     io::VecIo,
//!     memory::MemoryAddress,
//!     states::RunOutcome,
//! };
//!
//! let addr = MemoryAddress::new;
//! // Read a number, add 5, print it, stop.
//! let program = [
//!     Instruction::new(Opcode::Input, addr(0)).encode().value(),
//!     Instruction::new(Opcode::Add, addr(4)).encode().value(),
//!     Instruction::new(Opcode::Output, addr(0)).encode().value(),
//!     Instruction::new(Opcode::Halt, addr(0)).encode().value(),
//!     5,
//! ];
//!
//! let mut vm = MarieVM::new(VecIo::new([37]));
//! vm.load_program(addr(0), &program).unwrap();
//!
//! let RunOutcome::Terminated(vm) = vm.boot().run() else {
//!     panic!("program should halt");
//! };
//! assert_eq!(vm.io().outputs, vec![42]);
//! ```
//!
//! [MARIE.js]: https://marie.js.org

use std::marker::PhantomData;
use std::task::Poll;

use mrs_core::{Instruction, MemoryAddress, MemoryImage, Opcode, Value};

use crate::{
    alu::Alu,
    breakpoints::BreakpointSet,
    history::{Control, Effect, Entry, History, StepBackError},
    io::MarieVmIODevice,
    memory::{Memory, ProgramTooLarge},
    microcode::MicroOp,
    registers::{Register, Registers},
    states::{
        Fault, MarieVmState, MicroStepOutcome, Ready, RunOutcome, Running, StepOutcome, Stepping,
        SuspendReason, Terminated,
    },
};

pub mod alu;
pub mod breakpoints;
pub mod history;
pub mod io;
pub mod memory;
pub mod microcode;
pub mod registers;
pub mod states;

/// The instruction encoding, re-exported from [`mrs_core`].
pub mod instruction {
    pub use mrs_core::instruction::{Instruction, Opcode, SkipCondition};
}

/// The machine word type, re-exported from [`mrs_core`].
pub mod value {
    pub use mrs_core::value::Value;
}

/// The MARIE Virtual Machine.
#[must_use]
pub struct MarieVM<IO, S> {
    core: MarieVMCore<IO>,
    _state: PhantomData<fn() -> S>,
}

// Universal methods for all states
impl<IO, S> MarieVM<IO, S> {
    fn transition<S2: MarieVmState>(self) -> MarieVM<IO, S2> {
        MarieVM {
            core: self.core,
            _state: PhantomData,
        }
    }

    /// Returns the VM's registers.
    pub fn registers(&self) -> &Registers {
        &self.core.registers
    }

    /// Returns the VM's registers for modification.
    pub fn registers_mut(&mut self) -> &mut Registers {
        &mut self.core.registers
    }

    /// Returns the VM's memory.
    pub fn memory(&self) -> &Memory {
        &self.core.memory
    }

    /// Returns the VM's memory for modification.
    pub fn memory_mut(&mut self) -> &mut Memory {
        &mut self.core.memory
    }

    /// Returns the VM's I/O device.
    pub fn io(&self) -> &IO {
        &self.core.io_device
    }

    /// Returns the VM's I/O device for modification.
    ///
    /// Use this to feed a device that reported [`Poll::Pending`] before resuming.
    pub fn io_mut(&mut self) -> &mut IO {
        &mut self.core.io_device
    }

    /// Consumes the VM and returns its I/O device.
    pub fn into_io(self) -> IO {
        self.core.io_device
    }

    /// Returns the addresses the VM will suspend at.
    ///
    /// Breakpoints are only honoured by [`MarieVM::run`] and [`MarieVM::run_bounded`];
    /// stepping always advances.
    pub fn breakpoints(&self) -> &BreakpointSet {
        &self.core.breakpoints
    }

    /// Returns the breakpoint set for modification.
    pub fn breakpoints_mut(&mut self) -> &mut BreakpointSet {
        &mut self.core.breakpoints
    }

    /// Returns the journal of executed micro-operations.
    pub fn history(&self) -> &History {
        &self.core.history
    }

    /// Sets how many micro-operations are retained for stepping backwards.
    ///
    /// Recording is off by default, because a free-running program would otherwise
    /// grow the journal without bound. A limit of zero disables it again and discards
    /// what has been recorded.
    ///
    /// One instruction costs between six and ten entries, so a limit of a few thousand
    /// covers a comfortable debugging window.
    pub fn set_history_limit(&mut self, limit: usize) {
        self.core.history.set_limit(limit);
    }

    /// Discards recorded history without changing the limit.
    ///
    /// Useful for pinning the rewind boundary at an interesting point, so that
    /// stepping backwards cannot run past it.
    pub fn clear_history(&mut self) {
        self.core.history.clear();
    }

    /// Returns the instruction currently being executed, if one has been decoded.
    ///
    /// At an [instruction boundary](MarieVM::at_instruction_boundary) this is the
    /// instruction that just finished; part-way through a cycle it is the one in
    /// progress. Use [`MarieVM::next_instruction`] for the one about to run.
    pub fn decoded_instruction(&self) -> Option<Instruction> {
        self.core.decoded
    }

    /// Returns the address the [decoded instruction](MarieVM::decoded_instruction) was
    /// fetched from.
    pub fn instruction_address(&self) -> MemoryAddress {
        self.core.instruction_address
    }

    /// Decodes the word the program counter points at, without executing anything.
    ///
    /// Returns `None` if that word does not decode to a valid instruction. This is
    /// only the instruction that will run next if the machine is at an instruction
    /// boundary.
    pub fn next_instruction(&self) -> Option<Instruction> {
        self.core.peek(self.core.registers.pc)
    }

    /// Returns the micro-operation that will be executed next.
    ///
    /// Returns `None` only if the machine is part-way through an instruction that was
    /// never decoded, which cannot happen through the public API.
    pub fn next_micro_op(&self) -> Option<MicroOp> {
        self.core.current_micro_op()
    }

    /// Returns the index of the next micro-operation within the current cycle.
    ///
    /// Zero means the machine is between instructions.
    pub fn micro_pc(&self) -> u8 {
        self.core.micro_pc
    }

    /// Returns `true` if the machine is between instructions rather than part-way
    /// through one.
    pub fn at_instruction_boundary(&self) -> bool {
        self.core.micro_pc == 0
    }
}

// Methods which only apply to the MARIE VM when it is in the Ready state.
impl<IO> MarieVM<IO, Ready>
where
    IO: MarieVmIODevice,
{
    /// Creates a new instance of the MARIE Virtual Machine with the provided I/O device.
    pub fn new(io_device: IO) -> MarieVM<IO, Ready> {
        Self {
            core: MarieVMCore::new(io_device),
            _state: PhantomData,
        }
    }

    /// Creates a new instance with a provided program
    ///
    /// - `io_device`: The I/O device to be used by the VM.
    /// - `program_memory`: The program to be loaded into the VM's memory.
    /// - `entry_point`: The memory address where execution should begin.
    ///
    /// A note on the `entry_point`: The entry point is the memory address where the program's
    /// execution will start. It should be a valid address within the bounds of the VM's memory.
    /// This value is derived from the asm's first ORG directive **OR** `0x000` if there are no ORG
    /// directives in the program. If the entry point is not set correctly, the VM may attempt to
    /// execute invalid instructions or access memory out of bounds, leading to undefined behavior.
    pub fn new_with_program(
        io_device: IO,
        program_memory: &MemoryImage,
        entry_point: MemoryAddress,
    ) -> MarieVM<IO, Ready> {
        let mut vm = Self::new(io_device);
        vm.flash_program(program_memory, entry_point);
        vm
    }

    /// Flash the VM with a new program and set the entry point.
    pub fn flash_program(&mut self, program_memory: &MemoryImage, entry_point: MemoryAddress) {
        self.core.memory.flash(program_memory);
        self.core.rewind_to(entry_point);
    }

    /// Flash the VM's memory directly
    ///
    /// WARNING: If you want to flash it with a new **PROGRAM**, use [`Self::flash_program`] instead.
    pub fn flash_memory(&mut self, memory: &MemoryImage) {
        self.core.memory.flash(memory);
    }

    /// Loads a program that is shorter than the whole address space at `origin`, and sets
    /// the entry point to `origin`.
    ///
    /// Memory outside the program is left as it was; call [`Self::reset`] first for a
    /// clean machine.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramTooLarge`] if the program would run past the end of memory, in
    /// which case neither memory nor the program counter is modified.
    pub fn load_program(
        &mut self,
        origin: MemoryAddress,
        program: &[i16],
    ) -> Result<(), ProgramTooLarge> {
        self.core.memory.load(origin, program)?;
        self.core.rewind_to(origin);
        Ok(())
    }

    /// Resets the VM to its initial state, clearing memory and resetting registers.
    ///
    /// This is effectively the same as creating a new instance of the VM, but it retains the I/O
    /// device. Breakpoints and the history limit are retained too; recorded history is
    /// discarded, since it describes a machine that no longer exists.
    pub fn reset(&mut self) {
        self.core.reset();
    }

    /// Boot the VM, transitioning it from the Ready state to the Running state.
    pub fn boot(self) -> MarieVM<IO, Running> {
        self.transition::<Running>()
    }

    /// Boot the VM in debug mode, transitioning it from the Ready state to the Stepping state.
    ///
    /// To be able to step backwards as well as forwards, set a history limit with
    /// [`MarieVM::set_history_limit`] first.
    pub fn debug(self) -> MarieVM<IO, Stepping> {
        self.transition::<Stepping>()
    }
}

impl<IO: MarieVmIODevice> MarieVM<IO, Running> {
    /// Runs until the program halts, faults, hits a breakpoint, or blocks on input.
    ///
    /// This does not terminate for a program that loops forever; use
    /// [`MarieVM::run_bounded`] if the program is not trusted to halt.
    ///
    /// At least one instruction is always executed, so a `RunOutcome::Suspended` from a
    /// breakpoint can be resumed by simply calling `run` again.
    pub fn run(self) -> RunOutcome<IO> {
        self.run_inner(None)
    }

    /// Runs like [`MarieVM::run`], but executes at most `max_steps` instructions.
    ///
    /// Returns [`SuspendReason::StepLimit`] if the budget runs out; call again to
    /// continue.
    pub fn run_bounded(self, max_steps: u64) -> RunOutcome<IO> {
        self.run_inner(Some(max_steps))
    }

    fn run_inner(mut self, budget: Option<u64>) -> RunOutcome<IO> {
        let mut executed: u64 = 0;
        loop {
            if budget.is_some_and(|limit| executed >= limit) {
                return RunOutcome::Suspended(self, SuspendReason::StepLimit);
            }
            // Breakpoints only make sense between instructions. The first instruction
            // is exempt so that resuming from a breakpoint makes progress instead of
            // immediately re-reporting it.
            if executed > 0
                && self.core.micro_pc == 0
                && self.core.breakpoints.contains(self.core.registers.pc)
            {
                let address = self.core.registers.pc;
                return RunOutcome::Suspended(self, SuspendReason::Breakpoint(address));
            }

            match self.core.step() {
                Continuation::Continue => executed += 1,
                Continuation::Halted => return RunOutcome::Terminated(self.transition()),
                Continuation::AwaitingInput => {
                    return RunOutcome::Suspended(self, SuspendReason::AwaitingInput);
                }
                Continuation::Faulted(fault) => {
                    return RunOutcome::Faulted(self.transition(), fault);
                }
            }
        }
    }

    /// Hands the VM to the debugger without executing anything.
    pub fn pause(self) -> MarieVM<IO, Stepping> {
        self.transition::<Stepping>()
    }
}

impl<IO: MarieVmIODevice> MarieVM<IO, Stepping> {
    /// Executes instructions until the next instruction boundary.
    ///
    /// Breakpoints are ignored: stepping is already under debugger control. If the
    /// machine is part-way through an instruction — after
    /// [`MarieVM::micro_step`] or a stalled `Input` — this finishes that instruction
    /// rather than starting a new one.
    pub fn step(mut self) -> StepOutcome<IO> {
        match self.core.step() {
            Continuation::Continue => StepOutcome::Stepped(self),
            Continuation::Halted => StepOutcome::Terminated(self.transition()),
            Continuation::AwaitingInput => StepOutcome::AwaitingInput(self),
            Continuation::Faulted(fault) => StepOutcome::Faulted(self.transition(), fault),
        }
    }

    /// Executes exactly one micro-operation.
    pub fn micro_step(mut self) -> MicroStepOutcome<IO> {
        match self.core.micro_step() {
            MicroOutcome::Executed(micro_op) => MicroStepOutcome::Stepped(self, micro_op),
            MicroOutcome::Halted => MicroStepOutcome::Terminated(self.transition()),
            MicroOutcome::AwaitingInput => MicroStepOutcome::AwaitingInput(self),
            MicroOutcome::Faulted(fault) => MicroStepOutcome::Faulted(self.transition(), fault),
        }
    }

    /// Reverses the most recent micro-operation.
    ///
    /// Returns the operation that was undone.
    ///
    /// # Errors
    ///
    /// Returns [`StepBackError::NoHistory`] if nothing is recorded — history is off by
    /// default, see [`MarieVM::set_history_limit`] — or one of the irreversible-I/O
    /// variants if the operation exchanged a value with a device that cannot rewind.
    /// The machine is left untouched in every error case.
    pub fn micro_step_back(&mut self) -> Result<MicroOp, StepBackError> {
        self.core.micro_step_back()
    }

    /// Reverses micro-operations until the machine is back at an instruction boundary.
    ///
    /// From a boundary this undoes exactly one instruction. Part-way through an
    /// instruction it rewinds to the start of that instruction instead.
    ///
    /// Returns the number of micro-operations undone.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`MarieVM::micro_step_back`]. Because the operations
    /// are reversed one at a time, an error part-way through leaves the machine at
    /// that point rather than where it started; the state is still consistent, and
    /// [`MarieVM::micro_pc`] reports where it stopped.
    pub fn step_back(&mut self) -> Result<usize, StepBackError> {
        let mut undone = 0;
        loop {
            self.core.micro_step_back()?;
            undone += 1;
            if self.core.micro_pc == 0 {
                return Ok(undone);
            }
        }
    }

    /// Returns to free-running execution.
    pub fn resume(self) -> MarieVM<IO, Running> {
        self.transition::<Running>()
    }
}

impl<IO: MarieVmIODevice> MarieVM<IO, Terminated> {
    /// Hands a stopped machine back to the debugger without clearing it.
    ///
    /// The machine is positioned at the operation that stopped it, so stepping forward
    /// halts or faults again, and [`MarieVM::step_back`] rewinds into the program that
    /// led there.
    pub fn debug(self) -> MarieVM<IO, Stepping> {
        self.transition::<Stepping>()
    }

    /// Clears memory and registers, returning a machine ready to be programmed again.
    pub fn reset(self) -> MarieVM<IO, Ready> {
        let mut vm = self.transition::<Ready>();
        vm.core.reset();
        vm
    }

    /// Clears the machine and flashes a new program in one step.
    pub fn reset_with_program(
        self,
        program_memory: &MemoryImage,
        entry_point: MemoryAddress,
    ) -> MarieVM<IO, Ready> {
        let mut vm = self.reset();
        vm.flash_program(program_memory, entry_point);
        vm
    }
}

/// What the core wants the caller to do after executing one instruction.
enum Continuation {
    /// The instruction completed; keep going.
    Continue,
    /// The instruction was a `Halt`.
    Halted,
    /// An `Input` instruction found the device not ready. The machine is suspended at
    /// the micro-operation that reads it.
    AwaitingInput,
    /// The instruction could not be executed.
    Faulted(Fault),
}

/// What the core wants the caller to do after executing one micro-operation.
enum MicroOutcome {
    /// The operation completed.
    Executed(MicroOp),
    /// The operation was `Halt`.
    Halted,
    /// The operation reads the device, which was not ready.
    AwaitingInput,
    /// The operation could not be executed.
    Faulted(Fault),
}

/// The MARIE Virtual Machine **Core**.
struct MarieVMCore<IO> {
    registers: Registers,
    alu: Alu,
    memory: Memory,
    io_device: IO,
    breakpoints: BreakpointSet,
    history: History,
    /// Index of the next micro-operation within the current cycle.
    micro_pc: u8,
    /// The instruction being executed, once it has been decoded.
    decoded: Option<Instruction>,
    /// The address `decoded` was fetched from.
    instruction_address: MemoryAddress,
    /// The `Skipcond` comparison latch, set by [`MicroOp::Compare`].
    comparison: bool,
}

// Methods that do not touch the I/O device, and so need no bound on `IO`.
impl<IO> MarieVMCore<IO> {
    /// Decodes the word at `address` without disturbing the machine.
    fn peek(&self, address: MemoryAddress) -> Option<Instruction> {
        Instruction::decode(Value::new(self.memory.read(address)))
    }

    /// Returns the micro-operation at the current micro-program counter.
    fn current_micro_op(&self) -> Option<MicroOp> {
        let index = usize::from(self.micro_pc);
        match crate::microcode::FETCH_DECODE.get(index) {
            Some(op) => Some(*op),
            // Past the fetch phase, the decoded instruction selects the program.
            None => crate::microcode::execute(self.decoded?.opcode())
                .get(index - crate::microcode::FETCH_DECODE.len())
                .copied(),
        }
    }

    /// The number of micro-operations in the cycle currently being executed.
    fn cycle_length(&self) -> usize {
        match self.decoded {
            Some(instruction) => crate::microcode::cycle_length(instruction.opcode()),
            None => crate::microcode::FETCH_DECODE.len(),
        }
    }

    /// Snapshots the control state, for the undo journal.
    fn control(&self) -> Control {
        Control {
            micro_pc: self.micro_pc,
            instruction_address: self.instruction_address,
            decoded: self.decoded,
            comparison: self.comparison,
        }
    }

    /// Restores a control-state snapshot.
    fn restore(&mut self, control: Control) {
        self.micro_pc = control.micro_pc;
        self.instruction_address = control.instruction_address;
        self.decoded = control.decoded;
        self.comparison = control.comparison;
    }

    /// Positions the machine at the start of a fresh instruction at `entry_point`.
    fn rewind_to(&mut self, entry_point: MemoryAddress) {
        self.registers.pc = entry_point;
        self.micro_pc = 0;
        self.decoded = None;
        self.instruction_address = entry_point;
        self.comparison = false;
        // The journal describes a program that is no longer loaded.
        self.history.clear();
    }
}

impl<IO> MarieVMCore<IO>
where
    IO: MarieVmIODevice,
{
    /// Creates a new instance of the MARIE VM core with the provided I/O device.
    fn new(io_device: IO) -> Self {
        Self {
            registers: Registers::new(),
            alu: Alu,
            memory: Memory::new(),
            io_device,
            breakpoints: BreakpointSet::new(),
            history: History::default(),
            micro_pc: 0,
            decoded: None,
            instruction_address: MemoryAddress::ZERO,
            comparison: false,
        }
    }

    /// Resets the VM core to its initial state, clearing memory and resetting registers.
    /// Retains the IO device, breakpoints and history limit.
    fn reset(&mut self) {
        self.registers.reset();
        self.memory.clear();
        self.rewind_to(MemoryAddress::ZERO);
    }

    /// Runs micro-operations until the end of the current instruction.
    fn step(&mut self) -> Continuation {
        loop {
            match self.micro_step() {
                MicroOutcome::Executed(_) => {
                    if self.micro_pc == 0 {
                        return Continuation::Continue;
                    }
                }
                MicroOutcome::Halted => return Continuation::Halted,
                MicroOutcome::AwaitingInput => return Continuation::AwaitingInput,
                MicroOutcome::Faulted(fault) => return Continuation::Faulted(fault),
            }
        }
    }

    /// Executes a single micro-operation.
    fn micro_step(&mut self) -> MicroOutcome {
        let control = self.control();
        if self.micro_pc == 0 {
            // A new instruction begins here.
            self.instruction_address = self.registers.pc;
        }

        let Some(micro_op) = self.current_micro_op() else {
            // Only reachable if `decoded` were cleared part-way through a cycle, which
            // the public API does not allow.
            unreachable!("micro-program counter out of range for the decoded instruction");
        };

        match self.execute_micro_op(micro_op) {
            Ok(effect) => {
                self.history.record(Entry::new(control, micro_op, effect));
                self.advance_micro_pc();
                MicroOutcome::Executed(micro_op)
            }
            Err(stop) => {
                // Nothing was recorded and the micro-program counter has not moved, so
                // the stopped operation is retried if the machine is resumed.
                self.restore(control);
                stop
            }
        }
    }

    /// Moves to the next micro-operation, wrapping to zero at the end of the cycle.
    fn advance_micro_pc(&mut self) {
        let next = usize::from(self.micro_pc) + 1;
        self.micro_pc = if next >= self.cycle_length() {
            0
        } else {
            // `cycle_length` is bounded well below `u8::MAX`; see the microcode tests.
            next as u8
        };
    }

    /// Performs one micro-operation, returning what it changed or why it stopped.
    fn execute_micro_op(&mut self, micro_op: MicroOp) -> Result<Effect, MicroOutcome> {
        let effect = match micro_op {
            MicroOp::Transfer { target, source } => {
                self.write_register(target, self.registers.read(source))
            }
            MicroOp::ReadMemory => {
                let word = self.memory.read(self.registers.mar);
                self.write_register(Register::Mbr, word as u16)
            }
            MicroOp::WriteMemory => {
                let address = self.registers.mar;
                let previous = self.memory.read(address);
                self.memory.write(address, self.registers.mbr.value());
                Effect::Memory { address, previous }
            }
            MicroOp::IncrementPc => {
                self.write_register(Register::Pc, self.registers.pc.wrapping_add(1).value())
            }
            MicroOp::Decode => {
                let Some(instruction) = Instruction::decode(self.registers.ir) else {
                    return Err(MicroOutcome::Faulted(Fault::InvalidOpcode {
                        address: self.instruction_address,
                        word: self.registers.ir,
                    }));
                };
                self.decoded = Some(instruction);
                Effect::None
            }
            MicroOp::Add => {
                let sum = self.alu.add(self.registers.ac, self.registers.mbr);
                self.write_register(Register::Ac, sum.to_bits())
            }
            MicroOp::Subtract => {
                let difference = self.alu.sub(self.registers.ac, self.registers.mbr);
                self.write_register(Register::Ac, difference.to_bits())
            }
            MicroOp::LoadImmediate => {
                let immediate = self.alu.load_immediate(self.registers.ir);
                self.write_register(Register::Ac, immediate.to_bits())
            }
            MicroOp::Compare => {
                // Every two-bit encoding is a valid condition, so this cannot fail.
                let condition = self
                    .decoded
                    .and_then(Instruction::skip_condition)
                    .expect("Compare only appears in the Skipcond micro-program");
                self.comparison = self.alu.compare(self.registers.ac, condition);
                Effect::None
            }
            MicroOp::SkipIfComparison => {
                if self.comparison {
                    self.write_register(Register::Pc, self.registers.pc.wrapping_add(1).value())
                } else {
                    Effect::None
                }
            }
            MicroOp::ReadInput => match self.io_device.poll_input() {
                Poll::Pending => return Err(MicroOutcome::AwaitingInput),
                Poll::Ready(Err(error)) => {
                    return Err(MicroOutcome::Faulted(Fault::Io {
                        address: self.instruction_address,
                        opcode: Opcode::Input,
                        error,
                    }));
                }
                Poll::Ready(Ok(value)) => {
                    let previous = self.registers.read(Register::In);
                    self.registers.write(Register::In, value as u16);
                    Effect::InputRead { previous, value }
                }
            },
            MicroOp::WriteOutput => {
                let value = self.registers.output.value();
                if let Err(error) = self.io_device.output(value) {
                    return Err(MicroOutcome::Faulted(Fault::Io {
                        address: self.instruction_address,
                        opcode: Opcode::Output,
                        error,
                    }));
                }
                Effect::OutputWritten { value }
            }
            MicroOp::Halt => return Err(MicroOutcome::Halted),
        };
        Ok(effect)
    }

    /// Writes a register, returning the effect that reverses the write.
    fn write_register(&mut self, register: Register, bits: u16) -> Effect {
        let previous = self.registers.read(register);
        self.registers.write(register, bits);
        Effect::Register { register, previous }
    }

    /// Reverses the most recently executed micro-operation.
    fn micro_step_back(&mut self) -> Result<MicroOp, StepBackError> {
        let entry = *self.history.last().ok_or(StepBackError::NoHistory)?;

        // Each arm asks the device before touching the machine, so a refusal leaves
        // both the VM and the journal exactly as they were.
        match entry.effect() {
            Effect::None => {}
            Effect::Register { register, previous } => self.registers.write(register, previous),
            Effect::Memory { address, previous } => self.memory.write(address, previous),
            Effect::InputRead { previous, value } => {
                if !self.io_device.unread_input(value) {
                    return Err(StepBackError::IrreversibleInput);
                }
                self.registers.write(Register::In, previous);
            }
            Effect::OutputWritten { value } => {
                if !self.io_device.unwrite_output(value) {
                    return Err(StepBackError::IrreversibleOutput);
                }
            }
        }
        self.restore(entry.control);
        self.history.pop();
        Ok(entry.micro_op())
    }
}
