//! value type module

use std::fmt;
use std::ops::{Add, Neg, Sub};

/// Value type for the MARIE Virtual Machine (VM)
///
/// Internally, this is a word in MARIE (i16). MARIE.js stores registers as
/// unsigned 16-bit integers and interprets the sign from bit 15; an `i16` with
/// wrapping arithmetic is the same bit pattern with the same semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Value(i16);

impl Value {
    /// The zero value.
    pub const ZERO: Self = Self(0);

    /// Creates a new instance of the MARIE VM value type with the given i16 value.
    pub const fn new(value: i16) -> Self {
        Self(value)
    }

    /// Creates a value from a raw 16-bit pattern, reinterpreting it as signed.
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits as i16)
    }

    /// Returns the internal i16 value of the MARIE VM value type.
    pub const fn value(self) -> i16 {
        self.0
    }

    /// Returns the raw 16-bit pattern of this value.
    pub const fn to_bits(self) -> u16 {
        self.0 as u16
    }

    /// Returns the low 12 bits of this value (the address field of an instruction word).
    pub const fn low_12_bits(self) -> u16 {
        self.to_bits() & 0x0FFF
    }

    /// Returns `true` if this value is negative (bit 15 set).
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Returns `true` if this value is zero.
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Returns `true` if this value is strictly positive.
    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }
}

impl From<i16> for Value {
    fn from(value: i16) -> Self {
        Self(value)
    }
}

impl From<Value> for i16 {
    fn from(value: Value) -> Self {
        value.0
    }
}

impl Add for Value {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        Self(self.0.wrapping_add(other.0))
    }
}

impl Sub for Value {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        Self(self.0.wrapping_sub(other.0))
    }
}

impl Neg for Value {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(self.0.wrapping_neg())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::LowerHex for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.to_bits(), f)
    }
}

impl fmt::UpperHex for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.to_bits(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_sub_wrap_at_16_bits() {
        assert_eq!(Value::new(i16::MAX) + Value::new(1), Value::new(i16::MIN));
        assert_eq!(Value::new(i16::MIN) - Value::new(1), Value::new(i16::MAX));
    }

    #[test]
    fn bit_round_trip_preserves_pattern() {
        assert_eq!(Value::from_bits(0xFFFF), Value::new(-1));
        assert_eq!(Value::new(-1).to_bits(), 0xFFFF);
        assert_eq!(Value::from_bits(0x9123).low_12_bits(), 0x123);
    }

    #[test]
    fn sign_predicates_match_signed_interpretation() {
        assert!(Value::from_bits(0x8000).is_negative());
        assert!(Value::new(0).is_zero());
        assert!(Value::new(1).is_positive());
        assert!(!Value::new(0).is_positive());
    }
}
