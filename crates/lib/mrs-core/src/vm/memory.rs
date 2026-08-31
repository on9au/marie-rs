//! Memory module

/// Address space bit width for MARIE VM memory
pub const MEMORY_ADDRESS_SPACE_BIT_WIDTH: u16 = 12;

/// Word count for MARIE VM memory
pub const MEMORY_WORD_COUNT: u16 = 2_u16.pow(MEMORY_ADDRESS_SPACE_BIT_WIDTH as u32); // 4096 words, 12-bit address space

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

    /// Returns the underlying value of the memory address as usize
    pub fn value(&self) -> usize {
        self.0 as usize
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
        self.internal_memory[address.value()]
    }

    /// Writes a value to the specified memory address
    pub fn write(&mut self, address: MemoryAddress, value: i16) {
        self.internal_memory[address.value()] = value;
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
