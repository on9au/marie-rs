//! Memory module
//!
//! The address space itself lives in [`mrs_core::address`] so that the assembler and
//! linter can share it; this module adds the VM's mutable store and is re-exported
//! here for convenience.

use std::fmt;

pub use mrs_core::address::{ADDRESS_MASK, MEMORY_WORD_COUNT, MemoryAddress, MemoryImage};

/// An error returned when a program is too large to fit in memory at the requested origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramTooLarge {
    /// The origin the program was to be loaded at.
    pub origin: MemoryAddress,
    /// The length of the program, in words.
    pub length: usize,
}

impl fmt::Display for ProgramTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "program of {} words does not fit in {MEMORY_WORD_COUNT}-word memory at origin 0x{}",
            self.length, self.origin
        )
    }
}

impl std::error::Error for ProgramTooLarge {}

/// Memory for the MARIE Virtual Machine (VM)
#[derive(Clone, PartialEq, Eq)]
pub struct Memory {
    /// Internal memory storage
    internal_memory: MemoryImage,
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
        self.internal_memory[address.index()]
    }

    /// Writes a value to the specified memory address
    pub fn write(&mut self, address: MemoryAddress, value: i16) {
        self.internal_memory[address.index()] = value;
    }

    /// Clears the memory by setting all values to zero
    pub fn clear(&mut self) {
        self.internal_memory = [0; MEMORY_WORD_COUNT as usize];
    }

    /// Flash memory
    ///
    /// Note: If you are trying to flash the VM with a program,
    /// make sure you update the PC inside the registers too.
    pub fn flash(&mut self, memory: &MemoryImage) {
        self.internal_memory = *memory;
    }

    /// Copies `words` into memory starting at `origin`, leaving the rest of memory untouched.
    ///
    /// Unlike [`Memory::flash`], this does not require a full memory image.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramTooLarge`] if the program would run past the end of memory. Memory is
    /// left unmodified in that case.
    pub fn load(&mut self, origin: MemoryAddress, words: &[i16]) -> Result<(), ProgramTooLarge> {
        let start = origin.index();
        let end = start
            .checked_add(words.len())
            .filter(|end| *end <= MEMORY_WORD_COUNT as usize)
            .ok_or(ProgramTooLarge {
                origin,
                length: words.len(),
            })?;
        self.internal_memory[start..end].copy_from_slice(words);
        Ok(())
    }

    /// Returns the whole memory image as a slice.
    pub fn as_slice(&self) -> &[i16] {
        &self.internal_memory
    }

    /// Returns the whole memory image as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [i16] {
        &mut self.internal_memory
    }

    /// Returns a copy of the whole memory image.
    pub fn snapshot(&self) -> MemoryImage {
        self.internal_memory
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

// A derived `Debug` would print 4096 words; summarise instead.
impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let used = self.internal_memory.iter().filter(|w| **w != 0).count();
        f.debug_struct("Memory")
            .field("words", &MEMORY_WORD_COUNT)
            .field("non_zero_words", &used)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_copies_at_origin_and_bounds_checks() {
        let mut memory = Memory::new();
        memory.load(MemoryAddress::new(0x010), &[1, 2, 3]).unwrap();
        assert_eq!(memory.read(MemoryAddress::new(0x010)), 1);
        assert_eq!(memory.read(MemoryAddress::new(0x012)), 3);
        assert_eq!(memory.read(MemoryAddress::new(0x013)), 0);

        let err = memory.load(MemoryAddress::MAX, &[1, 2]).unwrap_err();
        assert_eq!(err.length, 2);
        // Memory is untouched on failure.
        assert_eq!(memory.read(MemoryAddress::MAX), 0);
    }
}
