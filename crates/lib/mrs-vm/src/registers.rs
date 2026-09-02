//! MARIE VM registers module

use crate::{memory::MemoryAddress, value::Value};

/// Registers of the MARIE Virtual Machine (VM)
pub struct Registers {
    /// Accumulator register
    ///
    /// Holds intermediate arithmetic and logic results, as well as data to be stored in memory.
    pub ac: Value,
    /// Program counter register
    ///
    /// Holds the memory address of the next instruction to be executed.
    pub pc: MemoryAddress,
    /// Instruction register
    ///
    /// Holds the current instruction being executed.
    pub ir: Value,
    /// Memory address register
    ///
    /// Holds the memory address that is currently being accessed (read from or written to) in
    /// memory.
    pub mar: MemoryAddress,
    /// Memory buffer register
    ///
    /// Holds data that was just read from or is waiting to be written to memory.
    pub mbr: Value,
}

impl Registers {
    /// Creates a new instance of the MARIE VM registers with default values.
    pub fn new() -> Self {
        Self {
            ac: Value::new(0),
            pc: MemoryAddress::new(0),
            ir: Value::new(0),
            mar: MemoryAddress::new(0),
            mbr: Value::new(0),
        }
    }

    /// Resets the registers to their default values.
    pub fn reset(&mut self) {
        self.ac = Value::new(0);
        self.pc = MemoryAddress::new(0);
        self.ir = Value::new(0);
        self.mar = MemoryAddress::new(0);
        self.mbr = Value::new(0);
    }
}

impl Default for Registers {
    fn default() -> Self {
        Self::new()
    }
}
