//! The MARIE Virtual Machine (VM) crate
//!
//! Contains the main [`MarieVM`] and modules for the ALU, I/O, memory, and
//! registers.

use std::marker::PhantomData;

use crate::{
    alu::Alu,
    io::MarieVmIODevice,
    memory::{MEMORY_WORD_COUNT, Memory, MemoryAddress, MemoryImage},
    registers::Registers,
    states::{Halted, MarieVmState, Running, Stepping},
};

pub mod alu;
pub mod io;
pub mod memory;
pub mod registers;
pub mod states;
mod value;

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
}

// Methods which only apply to the MARIE VM when it is in the Halted state.
impl<IO> MarieVM<IO, Halted>
where
    IO: MarieVmIODevice,
{
    /// Creates a new instance of the MARIE Virtual Machine with the provided I/O device.
    pub fn new(io_device: IO) -> MarieVM<IO, Halted> {
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
        program_memory: &[i16; MEMORY_WORD_COUNT as usize],
        entry_point: MemoryAddress,
    ) -> MarieVM<IO, Halted> {
        let mut vm = Self::new(io_device);
        vm.core.memory.flash(program_memory);
        vm.core.registers.pc = entry_point;
        vm
    }

    /// Flash the VM with a new program and set the entry point.
    pub fn flash_program(&mut self, program_memory: &MemoryImage, entry_point: MemoryAddress) {
        self.core.memory.flash(program_memory);
        self.core.registers.pc = entry_point;
    }

    /// Flash the VM's memory directly
    ///
    /// WARNING: If you want to flash it with a new **PROGRAM**, use [`Self::flash_program`] instead.
    pub fn flash_memory(&mut self, memory: &MemoryImage) {
        self.core.memory.flash(memory);
    }

    /// Resets the VM to its initial state, clearing memory and resetting registers.
    ///
    /// This is effectively the same as creating a new instance of the VM, but it retains the I/O
    /// device.
    pub fn reset(&mut self) {
        self.core.reset();
    }

    /// Boot the VM, transitioning it from the Halted state to the Running state.
    pub fn boot(self) -> MarieVM<IO, Running> {
        self.transition::<Running>()
    }

    /// Boot the VM in debug mode, transitioning it from the Halted state to the Stepping state.
    pub fn debug(self) -> MarieVM<IO, Stepping> {
        self.transition::<Stepping>()
    }
}

/// The MARIE Virtual Machine **Core**.
struct MarieVMCore<IO> {
    registers: Registers,
    alu: Alu,
    memory: Memory,
    io_device: IO,
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
        }
    }

    /// Resets the VM core to its initial state, clearing memory and resetting registers.
    /// Retains the IO device.
    fn reset(&mut self) {
        self.registers.reset();
        self.memory.clear();
    }
}
