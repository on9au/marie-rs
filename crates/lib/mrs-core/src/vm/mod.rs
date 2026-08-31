//! The MARIE Virtual Machine (VM) module
//!
//! Contains the main [`MarieVM`] and submodules for the ALU, control unit, I/O, memory, and
//! registers.

use crate::vm::alu::Alu;

pub mod alu;
pub mod control_unit;
pub mod io;
pub mod memory;
pub mod registers;

/// The MARIE Virtual Machine
pub struct MarieVM {
    _alu: Alu,
}
