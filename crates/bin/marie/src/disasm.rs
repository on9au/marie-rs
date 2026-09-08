//! `marie disasm` — read a binary image back as assembly.
//!
//! An image has no labels, no comments and no line numbers, so this cannot reconstruct
//! the source it came from. What it can do is say what each word decodes to, which is
//! what you want when handed a `.bin` and asked what it is.

use std::path::PathBuf;

use clap::Args as ClapArgs;
use mrs_core::{Instruction, MemoryAddress, Value, image};

use crate::input::parse_origin;
use crate::name;

/// Arguments to `marie disasm`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The binary image to read.
    pub file: PathBuf,

    /// The address the image starts at.
    #[arg(long, value_name = "ADDR", default_value = "000")]
    pub origin: String,

    /// Print every word, including the trailing zeros.
    ///
    /// A full image is mostly empty, so by default the dump stops after the last
    /// non-zero word.
    #[arg(short, long)]
    pub all: bool,
}

/// Disassembles the file.
pub fn run(args: Args) -> miette::Result<()> {
    let bytes = std::fs::read(&args.file)
        .map_err(|error| miette::miette!("could not read {}: {error}", args.file.display()))?;
    let words = image::decode_words(&bytes).map_err(|error| {
        miette::miette!(
            help = "expected little-endian 16-bit words, as MARIE.js writes",
            "{}: {error}",
            name(&args.file)
        )
    })?;
    let origin = parse_origin(&args.origin)?;

    // Trailing zeros are the unused rest of the address space, not program text.
    let end = if args.all {
        words.len()
    } else {
        words
            .iter()
            .rposition(|word| *word != 0)
            .map_or(0, |i| i + 1)
    };
    if end == 0 {
        println!("{}: empty image ({} words)", name(&args.file), words.len());
        return Ok(());
    }

    for (offset, word) in words[..end].iter().enumerate() {
        let Some(address) = MemoryAddress::try_new((origin.value() as usize + offset) as u16)
        else {
            return Err(miette::miette!(
                "the image runs past the end of memory from origin {origin}"
            ));
        };
        println!("{address}  {}", describe(*word));
    }
    if !args.all && end < words.len() {
        println!(
            "... {} zero words omitted (use --all to show)",
            words.len() - end
        );
    }
    Ok(())
}

/// Renders one word as its hexadecimal value and what it decodes to.
///
/// Every word is also a number, so the decimal value is shown as well: a word that is
/// data reads as nonsense as an instruction, and the reader needs both to tell which
/// it is.
pub fn describe(word: i16) -> String {
    let bits = word as u16;
    match Instruction::decode(Value::new(word)) {
        Some(instruction) => format!("{bits:04X}  {instruction:<14} ({word})"),
        // Only opcode 0xF is unassigned.
        None => format!("{bits:04X}  {:<14} ({word})", "<invalid>"),
    }
}
