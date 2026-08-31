//! MARIE VM registers module

use crate::vm::memory::MemoryAddress;

/// Registers of the MARIE Virtual Machine (VM)
pub struct Registers {
    /// Accumulator register
    ///
    /// Holds intermediate arithmetic and logic results, as well as data to be stored in memory.
    pub ac: i16,
    /// Program counter register
    ///
    /// Holds the memory address of the next instruction to be executed.
    pub pc: MemoryAddress,
    /// Instruction register
    ///
    /// Holds the current instruction being executed.
    pub ir: i16,
    /// Memory address register
    ///
    /// Holds the memory address that is currently being accessed (read from or written to) in
    /// memory.
    pub mar: MemoryAddress,
    /// Memory buffer register
    ///
    /// Holds data that was just read from or is waiting to be written to memory.
    pub mbr: i16,
}

impl Registers {
    /// Creates a new instance of the MARIE VM registers with default values.
    pub fn new() -> Self {
        Self {
            ac: 0,
            pc: MemoryAddress::new(0),
            ir: 0,
            mar: MemoryAddress::new(0),
            mbr: 0,
        }
    }
}

impl Default for Registers {
    fn default() -> Self {
        Self::new()
    }
}
