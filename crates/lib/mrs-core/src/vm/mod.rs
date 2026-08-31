//! The MARIE Virtual Machine (VM) module
//!
//! Contains the main [`MarieVM`] and submodules for the ALU, control unit, I/O, memory, and
//! registers.

use crate::vm::{alu::Alu, io::MarieVmIODevice, memory::Memory};

pub mod alu;
pub mod control_unit;
pub mod io;
pub mod memory;
pub mod registers;
mod value;

/// The MARIE Virtual Machine
pub struct MarieVM<IO: MarieVmIODevice> {
    _alu: Alu,
    _memory: Memory,
    _io_device: IO,
}
