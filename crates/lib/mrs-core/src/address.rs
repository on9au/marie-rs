//! The MARIE address space.

use std::fmt;
use std::ops::{Add, Sub};

use crate::value::Value;

/// Word count for MARIE VM memory
pub const MEMORY_WORD_COUNT: u16 = 4096; // 12-bit address space

/// Mask that reduces an arbitrary 16-bit quantity to a valid memory address.
///
/// MARIE.js masks `PC` and `MAR` with `0xFFF` after every register transfer;
/// this is that mask.
pub const ADDRESS_MASK: u16 = MEMORY_WORD_COUNT - 1;

// `ADDRESS_MASK` is only a correct wrap for a power-of-two memory size.
const _: () = assert!(MEMORY_WORD_COUNT.is_power_of_two());

/// Storage type for a full MARIE memory image
pub type MemoryImage = [i16; MEMORY_WORD_COUNT as usize];

/// Newtype for memory addresses in the MARIE VM
///
/// Enforces that memory addresses are within the valid range of the MARIE VM's memory address
/// space. Every constructor either masks to 12 bits or rejects the input, so an existing
/// `MemoryAddress` is always a valid index into a [`MemoryImage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MemoryAddress(u16);

impl MemoryAddress {
    /// Address `0x000`.
    pub const ZERO: Self = Self(0);

    /// The highest addressable word, `0xFFF`.
    pub const MAX: Self = Self(ADDRESS_MASK);

    /// Creates a new `MemoryAddress`.
    ///
    /// # Panics
    ///
    /// Panics if `address` is outside the 12-bit address space. Use
    /// [`MemoryAddress::try_new`] to handle out-of-range input, or
    /// [`MemoryAddress::new_masked`] to wrap it.
    pub const fn new(address: u16) -> Self {
        assert!(address < MEMORY_WORD_COUNT, "Address out of bounds");
        Self(address)
    }

    /// Creates a new `MemoryAddress`, returning `None` if `address` is out of bounds.
    pub const fn try_new(address: u16) -> Option<Self> {
        if address < MEMORY_WORD_COUNT {
            Some(Self(address))
        } else {
            None
        }
    }

    /// Creates a new `MemoryAddress` by discarding all but the low 12 bits.
    ///
    /// This is the behaviour of the MARIE hardware, which physically has only 12
    /// address lines.
    pub const fn new_masked(address: u16) -> Self {
        Self(address & ADDRESS_MASK)
    }

    /// Returns the underlying value of the memory address
    pub const fn value(self) -> u16 {
        self.0
    }

    /// Adds `offset` to this address, wrapping within the 12-bit address space.
    pub const fn wrapping_add(self, offset: u16) -> Self {
        Self::new_masked(self.0.wrapping_add(offset))
    }

    /// Subtracts `offset` from this address, wrapping within the 12-bit address space.
    pub const fn wrapping_sub(self, offset: u16) -> Self {
        Self::new_masked(self.0.wrapping_sub(offset))
    }

    /// Returns this address as an index into a [`MemoryImage`].
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl Add for MemoryAddress {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        self.wrapping_add(other.value())
    }
}

impl Sub for MemoryAddress {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        self.wrapping_sub(other.value())
    }
}

impl From<Value> for MemoryAddress {
    fn from(value: Value) -> Self {
        // Only the low 12 bits of a word can address memory.
        Self::new_masked(value.to_bits())
    }
}

impl From<MemoryAddress> for u16 {
    fn from(address: MemoryAddress) -> Self {
        address.0
    }
}

impl From<MemoryAddress> for Value {
    /// Zero-extends the address into a full word.
    fn from(address: MemoryAddress) -> Self {
        Value::from_bits(address.0)
    }
}

impl fmt::Display for MemoryAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:03X}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_arithmetic_wraps_within_12_bits() {
        assert_eq!(MemoryAddress::ZERO.wrapping_sub(1), MemoryAddress::MAX);
        assert_eq!(MemoryAddress::MAX.wrapping_add(1), MemoryAddress::ZERO);
        // The `Sub` impl must not underflow the underlying u16.
        assert_eq!(
            MemoryAddress::new(2) - MemoryAddress::new(5),
            MemoryAddress::new(0xFFD)
        );
        assert_eq!(
            MemoryAddress::new(0xFFF) + MemoryAddress::new(2),
            MemoryAddress::new(1)
        );
    }

    #[test]
    fn address_from_value_takes_low_12_bits() {
        assert_eq!(
            MemoryAddress::from(Value::from_bits(0x9123)),
            MemoryAddress::new(0x123)
        );
        assert_eq!(
            MemoryAddress::from(Value::new(-1)),
            MemoryAddress::new(0xFFF)
        );
    }

    #[test]
    fn try_new_rejects_out_of_range() {
        assert_eq!(MemoryAddress::try_new(0xFFF), Some(MemoryAddress::MAX));
        assert_eq!(MemoryAddress::try_new(0x1000), None);
    }
}
