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
