//! ALU-related code

/// MARIE CPU ALU
/// Houses methods for performing arithmetic and logical operations
/// found in MARIE CPUs.
pub struct Alu;

impl Alu {
    /// Adds two numbers together
    pub fn add(x: i16, y: i16) -> i16 {
        x.wrapping_add(y)
    }

    /// Subtracts one number from another
    pub fn sub(x: i16, y: i16) -> i16 {
        x.wrapping_sub(y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(Alu::add(5, 3), 8);
        assert_eq!(Alu::add(-5, -3), -8);
        assert_eq!(Alu::add(-5, 3), -2);
    }

    #[test]
    fn test_sub() {
        assert_eq!(Alu::sub(5, 3), 2);
        assert_eq!(Alu::sub(-5, -3), -2);
        assert_eq!(Alu::sub(-5, 3), -8);
    }
}
