//! Memory module

use std::ops::{Add, Sub};

use crate::value::Value;

/// Word count for MARIE VM memory
pub const MEMORY_WORD_COUNT: u16 = 4096; // 12-bit address space

/// Newtype for memory addresses in the MARIE VM
///
/// Enforces that memory addresses are within the valid range of the MARIE VM's memory address
/// space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryAddress(u16);

impl MemoryAddress {
    /// Creates a new `MemoryAddress` from a `usize`
    pub fn new(address: u16) -> Self {
        assert!(address < MEMORY_WORD_COUNT, "Address out of bounds");
        Self(address)
    }

    /// Returns the underlying value of the memory address
    pub fn value(&self) -> u16 {
        self.0
    }
}

impl Add for MemoryAddress {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        // 12 bit wrapping add
        Self::new((self.value() + other.value()) & 0x0FFF)
    }
}

impl Sub for MemoryAddress {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        // 12 bit wrapping sub
        Self::new((self.value() - other.value()) & 0x0FFF)
    }
}

impl From<Value> for MemoryAddress {
    fn from(value: Value) -> Self {
        let u16_bits = value.value() as u16;
        MemoryAddress::new(u16_bits & 0x0FFF) // Ensure it's within 12 bits
    }
}

/// Memory for the MARIE Virtual Machine (VM)
pub struct Memory {
    /// Internal memory storage
    ///
    internal_memory: [i16; MEMORY_WORD_COUNT as usize],
}

impl Memory {
    /// Creates a new instance of the MARIE VM memory
    pub fn new() -> Self {
        Self {
            internal_memory: [0; MEMORY_WORD_COUNT as usize],
        }
    }

    /// Reads a value from the specified memory address
    pub fn read(&self, address: MemoryAddress) -> i16 {
        self.internal_memory[address.value() as usize]
    }

    /// Writes a value to the specified memory address
    pub fn write(&mut self, address: MemoryAddress, value: i16) {
        self.internal_memory[address.value() as usize] = value;
    }

    /// Clears the memory by setting all values to zero
    pub fn clear(&mut self) {
        self.internal_memory = [0; MEMORY_WORD_COUNT as usize];
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}
