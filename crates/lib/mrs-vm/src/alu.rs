//! ALU-related code

use crate::value::Value;

/// MARIE CPU ALU
/// Houses methods for performing arithmetic and logical operations
/// found in MARIE CPUs.
pub struct Alu;

impl Alu {
    /// Adds two numbers together
    pub fn add(x: Value, y: Value) -> Value {
        x + y
    }

    /// Subtracts one number from another
    pub fn sub(x: Value, y: Value) -> Value {
        x - y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(Alu::add(Value::new(5), Value::new(3)), Value::new(8));
        assert_eq!(Alu::add(Value::new(-5), Value::new(-3)), Value::new(-8));
        assert_eq!(Alu::add(Value::new(-5), Value::new(3)), Value::new(-2));
    }

    #[test]
    fn test_sub() {
        assert_eq!(Alu::sub(Value::new(5), Value::new(3)), Value::new(2));
        assert_eq!(Alu::sub(Value::new(-5), Value::new(-3)), Value::new(-2));
        assert_eq!(Alu::sub(Value::new(-5), Value::new(3)), Value::new(-8));
    }
}
