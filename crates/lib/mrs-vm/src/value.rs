//! value type module

use std::ops::{Add, Sub};

/// Value type for the MARIE Virtual Machine (VM)
///
/// Internally, this is a word in MARIE (i16)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Value(i16);

impl Value {
    /// Creates a new instance of the MARIE VM value type with the given i16 value.
    pub fn new(value: i16) -> Self {
        Self(value)
    }

    /// Returns the internal i16 value of the MARIE VM value type.
    pub fn value(&self) -> i16 {
        self.0
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
