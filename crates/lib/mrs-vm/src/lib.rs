//! The MARIE Virtual Machine (VM) crate
//!
//! Contains the main [`MarieVM`] and modules for the ALU, control unit, I/O, memory, and
//! registers.

use std::marker::PhantomData;

use crate::{
    alu::Alu,
    io::MarieVmIODevice,
    memory::{MEMORY_WORD_COUNT, Memory, MemoryAddress},
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
pub struct MarieVM<IO: MarieVmIODevice, S: MarieVmState> {
    _core: MarieVMCore<IO>,
    _state: PhantomData<S>,
}

// Methods which only apply to the MARIE VM when it is in the Halted state.
impl<IO> MarieVM<IO, Halted>
where
    IO: MarieVmIODevice,
{
    /// Creates a new instance of the MARIE Virtual Machine with the provided I/O device.
    pub fn new(io_device: IO) -> MarieVM<IO, Halted> {
        Self {
            _core: MarieVMCore::new(io_device),
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
        program_memory: [i16; MEMORY_WORD_COUNT as usize],
        entry_point: MemoryAddress,
    ) -> MarieVM<IO, Halted> {
        let mut vm = Self::new(io_device);
        vm._core._memory.flash(program_memory);
        vm._core._registers.pc = entry_point;
        vm
    }

    /// Flash the VM with a new program and set the entry point.
    pub fn flash_program(
        &mut self,
        program_memory: [i16; MEMORY_WORD_COUNT as usize],
        entry_point: MemoryAddress,
    ) {
        self._core._memory.flash(program_memory);
        self._core._registers.pc = entry_point;
    }

    /// Flash the VM's memory directly
    ///
    /// WARNING: If you want to flash it with a new **PROGRAM**, use [`Self::flash_program`] instead.
    pub fn flash_memory(&mut self, memory: [i16; MEMORY_WORD_COUNT as usize]) {
        self._core._memory.flash(memory);
    }

    /// Resets the VM to its initial state, clearing memory and resetting registers.
    ///
    /// This is effectively the same as creating a new instance of the VM, but it retains the I/O
    /// device.
    pub fn reset(&mut self) {
        self._core.reset();
    }

    /// Boot the VM, transitioning it from the Halted state to the Running state.
    #[must_use]
    pub fn boot(self) -> MarieVM<IO, Running> {
        MarieVM {
            _core: self._core,
            _state: PhantomData,
        }
    }

    /// Boot the VM in debug mode, transitioning it from the Halted state to the Stepping state.
    #[must_use]
    pub fn debug(self) -> MarieVM<IO, Stepping> {
        MarieVM {
            _core: self._core,
            _state: PhantomData,
        }
    }
}

/// The MARIE Virtual Machine **Core**.
struct MarieVMCore<IO: MarieVmIODevice> {
    _registers: Registers,
    _alu: Alu,
    _memory: Memory,
    _io_device: IO,
}

impl<IO> MarieVMCore<IO>
where
    IO: MarieVmIODevice,
{
    /// Creates a new instance of the MARIE Virtual Machine with the provided I/O device.
    fn new(io_device: IO) -> Self {
        Self {
            _registers: Registers::new(),
            _alu: Alu,
            _memory: Memory::new(),
            _io_device: io_device,
        }
    }

    /// Resets the VM core to its initial state, clearing memory and resetting registers.
    /// Retains the IO device.
    fn reset(&mut self) {
        self._registers.reset();
        self._alu = Alu;
        self._memory.clear();
    }
}
