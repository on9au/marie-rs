//! MARIE-rs core crate, for shared types and traits used by the MARIE-rs ecosystem
//!
//! This crate owns the *architecture*: the word type, the address space, the
//! instruction encoding, and the assembler directives. It knows nothing about
//! executing programs, so the assembler, the linter and the VM can all depend on it
//! without depending on each other.
//!
//! The modelled machine is [MARIE.js], which differs from the textbook (Null & Lobur)
//! MARIE in a few ways; see [`Opcode`] for the details.
//!
//! [MARIE.js]: https://marie.js.org

pub mod address;
pub mod directive;
pub mod instruction;
pub mod literal;
pub mod value;

pub use address::{ADDRESS_MASK, MEMORY_WORD_COUNT, MemoryAddress, MemoryImage};
pub use directive::Directive;
pub use instruction::{Instruction, Opcode, SkipCondition};
pub use literal::{ParseWordError, Radix};
pub use value::Value;
