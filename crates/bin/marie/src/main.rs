//! `marie` — assemble, run, lint and debug MARIE assembly.
//!
//! ```console
//! $ marie asm program.mas --listing
//! $ marie run program.mas
//! $ marie lint program.mas --deny falls-into-data
//! $ marie debug program.mas
//! ```
//!
//! Every subcommand reports problems through the same compiler-style renderer, so a
//! diagnostic looks the same whether it came from the assembler or a lint.

mod asm;
mod debug;
mod disasm;
mod display;
mod input;
mod interrupt;
mod lint;
mod run;
mod stdin;

use std::path::Path;

use clap::{Parser, Subcommand};

/// Assemble, run, lint and debug MARIE assembly.
#[derive(Debug, Parser)]
#[command(name = "marie", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Assemble a program, optionally writing a `.bin` memory image.
    Asm(asm::Args),
    /// Run a program: assembly, or a `.bin` memory image.
    Run(run::Args),
    /// Check a program for mistakes that assemble cleanly.
    Lint(lint::Args),
    /// Step through a program interactively.
    Debug(debug::Args),
    /// Disassemble a binary memory image.
    Disasm(disasm::Args),
}

fn main() -> miette::Result<()> {
    match Cli::parse().command {
        Command::Asm(args) => asm::run(args),
        Command::Run(args) => run::run(args),
        Command::Lint(args) => lint::run(args),
        Command::Debug(args) => debug::run(args),
        Command::Disasm(args) => disasm::run(args),
    }
}

/// Reads a source file, reporting a missing or unreadable one as a diagnostic.
pub fn read(path: &Path) -> miette::Result<String> {
    std::fs::read_to_string(path)
        .map_err(|error| miette::miette!("could not read {}: {error}", path.display()))
}

/// The name a file is reported under: its path as written.
pub fn name(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_tree_is_well_formed() {
        // Catches conflicting flags, bad defaults and duplicate short options.
        Cli::command().debug_assert();
    }
}
