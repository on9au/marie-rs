//! The language features, each a pure function over an analysed [`Document`].
//!
//! Nothing here does I/O or touches the protocol loop, so every feature is testable by
//! calling it with a position and comparing the result.
//!
//! [`Document`]: crate::document::Document

pub mod actions;
pub mod completion;
pub mod diagnostics;
pub mod hints;
pub mod hover;
pub mod navigation;
pub mod symbols;
pub mod tokens;
