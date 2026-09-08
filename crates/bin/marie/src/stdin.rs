//! How `Input` reads the terminal.
//!
//! `run` and `debug` both offer the choice and both spell it the same way, so the flag
//! lives here rather than being written out twice.

use clap::ValueEnum;

/// How a line typed at the `input>` prompt is read.
///
/// This is the command-line spelling of [`mrs_vm::io::InputMode`]; the VM's enum stays
/// free of `clap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum InputMode {
    /// One value per line: decimal, or `0x`, `0o` and `0b` for another base.
    #[default]
    Word,
    /// A line of text, spent one UTF-16 code unit per `Input`.
    Utf16,
}

impl From<InputMode> for mrs_vm::io::InputMode {
    fn from(mode: InputMode) -> Self {
        match mode {
            InputMode::Word => mrs_vm::io::InputMode::Word,
            InputMode::Utf16 => mrs_vm::io::InputMode::Utf16,
        }
    }
}
