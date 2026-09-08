//! A language server for MARIE assembly.
//!
//! Everything here is a thin layer over [`mrs_asm`] and [`mrs_lint`]: the assembler
//! already produces spans, a symbol table with definition and reference sites, a
//! two-way source map, classified tokens and extensible diagnostics, which is most of
//! what a language server is. This crate converts those into protocol types and runs
//! the message loop.
//!
//! # Features
//!
//! | Request | Built from |
//! |---|---|
//! | Diagnostics | assembler errors and lint findings |
//! | Hover | opcode descriptions, label addresses, assembled words |
//! | Go to definition, references, highlight, rename | [`mrs_asm::SymbolTable`] |
//! | Document symbols | labels, ordered by address |
//! | Completion | mnemonics, directives, labels, `Skipcond` conditions |
//! | Semantic tokens | [`mrs_asm::lexer::tokenize`] |
//! | Inlay hints | each line's address and assembled word |
//! | Quick fixes | mechanical rewrites for four findings |
//!
//! # Position encoding
//!
//! LSP columns count code units in a negotiated encoding, while every span in this
//! workspace is a byte offset. [`encoding`] does that conversion in one place; see its
//! documentation for why it is worth its own module.

pub mod document;
pub mod encoding;
pub mod features;
pub mod server;

use mrs_asm::diagnostic::Code;

pub use document::Document;
pub use encoding::{PositionEncoding, Positions};
pub use server::Server;

// Lint codes this crate offers fixes for. Re-stated here so the fix table reads as a
// list of codes rather than a chain of module paths.
use mrs_lint::lints::hazards::{MASKED_SKIPCOND, SKIPCOND_LABEL_OPERAND};
use mrs_lint::lints::style::NON_CANONICAL_MNEMONIC;

/// Every diagnostic code this server can offer a quick fix for.
pub const FIXABLE: [Code; 4] = [
    Code::UNKNOWN_LABEL,
    MASKED_SKIPCOND,
    SKIPCOND_LABEL_OPERAND,
    NON_CANONICAL_MNEMONIC,
];
