//! A read-only view of the memory-mapped display.
//!
//! The geometry and the pixel format live in [`mrs_core::display`]; this is the part
//! that reads them out of a running machine.
//!
//! Nothing here is stateful: the display *is* the top of memory, so a view is just a
//! borrow. A frontend that wants to be told about changes rather than poll for them
//! implements [`MarieVmIODevice::display_write`](crate::io::MarieVmIODevice::display_write).
//!
//! # Driving a frontend
//!
//! Because the hook lives on the I/O device, a frontend is just a device. It receives
//! pixels as they change and renders them however it likes — a canvas, an image, a
//! terminal — without the VM knowing anything about how it draws.
//!
//! ```
//! use std::task::Poll;
//! use mrs_core::display::Rgb555;
//! use mrs_vm::io::{IoError, MarieVmIODevice};
//! use mrs_vm::{MarieVM, states::RunOutcome};
//! use mrs_vm::instruction::{Instruction, Opcode};
//! use mrs_vm::memory::MemoryAddress;
//!
//! /// Collects pixel updates for something else to paint.
//! #[derive(Default)]
//! struct Frontend {
//!     updates: Vec<(usize, [u8; 3])>,
//! }
//!
//! impl MarieVmIODevice for Frontend {
//!     fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
//!         Poll::Ready(Err(IoError::Eof))
//!     }
//!     fn output(&mut self, _value: i16) -> Result<(), IoError> {
//!         Ok(())
//!     }
//!     fn display_write(&mut self, index: usize, pixel: Rgb555) {
//!         self.updates.push((index, pixel.to_rgb8()));
//!     }
//! }
//!
//! let addr = MemoryAddress::new;
//! // Load a colour, store it at 0xF00, halt.
//! let program = [
//!     Instruction::new(Opcode::Load, addr(3)).encode().value(),
//!     Instruction::new(Opcode::Store, addr(0xF00)).encode().value(),
//!     Instruction::new(Opcode::Halt, addr(0)).encode().value(),
//!     0x7C00u16 as i16, // pure red
//! ];
//!
//! let mut vm = MarieVM::new(Frontend::default());
//! vm.load_program(addr(0), &program).unwrap();
//! let RunOutcome::Terminated(vm) = vm.boot().run() else { panic!("should halt") };
//!
//! assert_eq!(vm.io().updates, vec![(0, [255, 0, 0])]);
//! // The same picture is also readable in full at any time.
//! assert_eq!(vm.display().pixel(0, 0).unwrap().to_rgb8(), [255, 0, 0]);
//! ```

use mrs_core::MemoryAddress;
use mrs_core::display::{DISPLAY_HEIGHT, DISPLAY_ORIGIN, DISPLAY_WIDTH, DISPLAY_WORDS, Rgb555};

use crate::memory::Memory;

/// A borrowed view of the display region.
#[derive(Debug, Clone, Copy)]
pub struct Display<'a> {
    memory: &'a Memory,
}

impl<'a> Display<'a> {
    /// Creates a view over `memory`.
    pub const fn new(memory: &'a Memory) -> Self {
        Self { memory }
    }

    /// Returns the pixel at a row-major index, or `None` past the end.
    pub fn at(&self, index: usize) -> Option<Rgb555> {
        if index >= DISPLAY_WORDS {
            return None;
        }
        let address = MemoryAddress::new(DISPLAY_ORIGIN.value() + index as u16);
        Some(Rgb555::from_bits(self.memory.read(address) as u16))
    }

    /// Returns the pixel at `(column, row)`.
    pub fn pixel(&self, column: usize, row: usize) -> Option<Rgb555> {
        mrs_core::display::address_of(column, row)
            .map(|address| Rgb555::from_bits(self.memory.read(address) as u16))
    }

    /// Iterates over every pixel, row-major.
    pub fn iter(&self) -> impl Iterator<Item = Rgb555> + '_ {
        (0..DISPLAY_WORDS).map(|index| self.at(index).unwrap_or_default())
    }

    /// Iterates over the rows, each an iterator of pixels.
    pub fn rows(&self) -> impl Iterator<Item = impl Iterator<Item = Rgb555> + '_> + '_ {
        (0..DISPLAY_HEIGHT).map(move |row| {
            (0..DISPLAY_WIDTH).map(move |column| self.pixel(column, row).unwrap_or_default())
        })
    }

    /// Copies the display into `[r, g, b]` triples, row-major.
    ///
    /// This is the shape a canvas, an image encoder or a web frontend wants.
    pub fn to_rgb8(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(DISPLAY_WORDS * 3);
        for pixel in self.iter() {
            bytes.extend_from_slice(&pixel.to_rgb8());
        }
        bytes
    }

    /// Copies the raw words, row-major.
    pub fn to_words(&self) -> Vec<u16> {
        self.iter().map(Rgb555::bits).collect()
    }

    /// Returns `true` if every pixel is black.
    pub fn is_blank(&self) -> bool {
        self.iter().all(Rgb555::is_black)
    }
}
