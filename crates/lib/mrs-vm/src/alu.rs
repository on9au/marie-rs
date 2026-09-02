//! ALU-related code

use crate::instruction::SkipCondition;
use crate::value::Value;

/// MARIE CPU ALU
///
/// Houses methods for performing the arithmetic, immediate-load and comparison
/// operations found in MARIE CPUs. All arithmetic is 16-bit and wraps on
/// overflow, matching the `& 0xFFFF` masking MARIE.js applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Alu;

impl Alu {
    /// Adds two numbers together, wrapping on overflow.
    pub fn add(&self, x: Value, y: Value) -> Value {
        x + y
    }

    /// Subtracts one number from another, wrapping on overflow.
    pub fn sub(&self, x: Value, y: Value) -> Value {
        x - y
    }

    /// Zero-extends the 12-bit address field of an instruction word into a full value.
    ///
    /// This is the `LoadImmi` datapath: `AC <- IR & 0xFFF`. The result is always in
    /// `0..=4095`, so it is never negative.
    pub fn load_immediate(&self, instruction_word: Value) -> Value {
        Value::from_bits(instruction_word.low_12_bits())
    }

    /// Evaluates a `Skipcond` condition against the accumulator.
    pub fn compare(&self, ac: Value, condition: SkipCondition) -> bool {
        condition.holds(ac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        let alu = Alu;
        assert_eq!(alu.add(Value::new(5), Value::new(3)), Value::new(8));
        assert_eq!(alu.add(Value::new(-5), Value::new(-3)), Value::new(-8));
        assert_eq!(alu.add(Value::new(-5), Value::new(3)), Value::new(-2));
    }

    #[test]
    fn test_sub() {
        let alu = Alu;
        assert_eq!(alu.sub(Value::new(5), Value::new(3)), Value::new(2));
        assert_eq!(alu.sub(Value::new(-5), Value::new(-3)), Value::new(-2));
        assert_eq!(alu.sub(Value::new(-5), Value::new(3)), Value::new(-8));
    }

    #[test]
    fn arithmetic_wraps_at_16_bits() {
        let alu = Alu;
        assert_eq!(
            alu.add(Value::new(i16::MAX), Value::new(1)),
            Value::new(i16::MIN)
        );
        assert_eq!(
            alu.sub(Value::new(i16::MIN), Value::new(1)),
            Value::new(i16::MAX)
        );
    }

    #[test]
    fn load_immediate_zero_extends_12_bits() {
        let alu = Alu;
        // Even though the word is negative as an i16, the immediate is unsigned.
        assert_eq!(
            alu.load_immediate(Value::from_bits(0xAFFF)),
            Value::new(4095)
        );
        assert_eq!(alu.load_immediate(Value::from_bits(0xA000)), Value::ZERO);
    }

    #[test]
    fn compare_delegates_to_the_condition() {
        let alu = Alu;
        assert!(alu.compare(Value::new(-1), SkipCondition::Negative));
        assert!(!alu.compare(Value::new(-1), SkipCondition::Zero));
    }
}
