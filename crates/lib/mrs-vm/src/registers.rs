//! MARIE VM registers module

use std::fmt;

use mrs_core::{MemoryAddress, Value};

/// Names one of the CPU's registers.
///
/// The micro-programs in [`crate::microcode`] are written in terms of this selector,
/// so a register transfer is data rather than code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Register {
    /// The accumulator.
    Ac,
    /// The program counter.
    Pc,
    /// The instruction register.
    Ir,
    /// The memory address register.
    Mar,
    /// The memory buffer register.
    Mbr,
    /// The input register.
    In,
    /// The output register.
    Out,
}

impl Register {
    /// Every register.
    pub const ALL: [Register; 7] = [
        Register::Ac,
        Register::Pc,
        Register::Ir,
        Register::Mar,
        Register::Mbr,
        Register::In,
        Register::Out,
    ];

    /// Returns `true` if this register is only 12 bits wide.
    ///
    /// `PC` and `MAR` address memory, so they hold 12 bits; writing a wider value
    /// discards the high nibble, exactly as MARIE.js does after every transfer.
    pub const fn is_address_register(self) -> bool {
        matches!(self, Register::Pc | Register::Mar)
    }

    /// Returns the register's name.
    pub const fn name(self) -> &'static str {
        match self {
            Register::Ac => "AC",
            Register::Pc => "PC",
            Register::Ir => "IR",
            Register::Mar => "MAR",
            Register::Mbr => "MBR",
            Register::In => "IN",
            Register::Out => "OUT",
        }
    }
}

impl fmt::Display for Register {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Registers of the MARIE Virtual Machine (VM)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
    /// Input register
    ///
    /// Holds the last value read from the input device, before it is transferred to the AC.
    pub input: Value,
    /// Output register
    ///
    /// Holds the last value written to the output device.
    pub output: Value,
}

impl Registers {
    /// Creates a new instance of the MARIE VM registers with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets the registers to their default values.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Reads a register as a raw bit pattern.
    ///
    /// The 12-bit registers are zero-extended.
    pub const fn read(&self, register: Register) -> u16 {
        match register {
            Register::Ac => self.ac.to_bits(),
            Register::Pc => self.pc.value(),
            Register::Ir => self.ir.to_bits(),
            Register::Mar => self.mar.value(),
            Register::Mbr => self.mbr.to_bits(),
            Register::In => self.input.to_bits(),
            Register::Out => self.output.to_bits(),
        }
    }

    /// Writes a raw bit pattern into a register.
    ///
    /// Writes to `PC` and `MAR` are masked to 12 bits.
    pub const fn write(&mut self, register: Register, bits: u16) {
        match register {
            Register::Ac => self.ac = Value::from_bits(bits),
            Register::Pc => self.pc = MemoryAddress::new_masked(bits),
            Register::Ir => self.ir = Value::from_bits(bits),
            Register::Mar => self.mar = MemoryAddress::new_masked(bits),
            Register::Mbr => self.mbr = Value::from_bits(bits),
            Register::In => self.input = Value::from_bits(bits),
            Register::Out => self.output = Value::from_bits(bits),
        }
    }
}

impl fmt::Display for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AC={ac:04X} PC={pc} IR={ir:04X} MAR={mar} MBR={mbr:04X} IN={input:04X} OUT={output:04X}",
            ac = self.ac,
            pc = self.pc,
            ir = self.ir,
            mar = self.mar,
            mbr = self.mbr,
            input = self.input,
            output = self.output,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_registers_mask_writes_to_12_bits() {
        let mut registers = Registers::new();
        for register in Register::ALL {
            registers.write(register, 0x9123);
            let expected = if register.is_address_register() {
                0x0123
            } else {
                0x9123
            };
            assert_eq!(registers.read(register), expected, "{register}");
        }
    }
}
