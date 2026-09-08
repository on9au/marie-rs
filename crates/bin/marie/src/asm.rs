//! `marie asm` — assemble a program.

use std::path::PathBuf;

use clap::Args as ClapArgs;
use mrs_asm::{Options, assemble_with};
use mrs_core::image;

use crate::{name, read};

/// Arguments to `marie asm`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The source file to assemble.
    pub file: PathBuf,

    /// Print an address/word/source listing.
    #[arg(short, long)]
    pub listing: bool,

    /// Print the assembled words as hexadecimal, one per line.
    #[arg(long, conflicts_with = "listing")]
    pub hex: bool,

    /// Also report lint findings.
    #[arg(long)]
    pub lint: bool,

    /// Write a binary memory image to this path.
    ///
    /// The format is the one MARIE.js downloads: little-endian 16-bit words. By
    /// default the whole 4096-word address space is written, with the program at its
    /// origin; `--bare` writes only the program's own words.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Write only the assembled words rather than a full memory image.
    #[arg(long, requires = "output")]
    pub bare: bool,
}

/// Assembles the file and prints whatever was asked for.
pub fn run(args: Args) -> miette::Result<()> {
    let source = read(&args.file)?;
    let options = if args.lint {
        Options::linting()
    } else {
        Options::bare()
    };
    let assembly = assemble_with(&source, options);

    if !assembly.succeeded() {
        return Err(assembly.report(name(&args.file), &source).into());
    }
    // Warnings do not stop the assembly, but they should still be seen.
    if assembly.warnings().count() > 0 {
        eprintln!(
            "{:?}",
            miette::Report::new(assembly.report(name(&args.file), &source))
        );
    }

    if let Some(path) = &args.output {
        let bytes = if args.bare {
            image::encode_words(&assembly.words)
        } else {
            // A full image is what MARIE.js writes, and what its own reader expects.
            let full = assembly
                .image()
                .ok_or_else(|| miette::miette!("the program does not fit in memory"))?;
            image::encode_image(&full)
        };
        std::fs::write(path, &bytes)
            .map_err(|error| miette::miette!("could not write {}: {error}", path.display()))?;
        println!("wrote {} bytes to {}", bytes.len(), path.display());
        return Ok(());
    }

    if args.listing {
        print_listing(&assembly, &source);
    } else if args.hex {
        for word in &assembly.words {
            println!("{:04X}", *word as u16);
        }
    } else {
        println!(
            "assembled {} words at {}",
            assembly.words.len(),
            assembly.origin
        );
    }
    Ok(())
}

/// Prints `address  word  source`, the shape an assembler listing has always had.
fn print_listing(assembly: &mrs_asm::Assembly, source: &str) {
    for (index, item) in assembly.program.items.iter().enumerate() {
        let word = assembly.words.get(index).copied().unwrap_or(0) as u16;
        let text = assembly
            .lines
            .line_span(item.line)
            .and_then(|span| span.text(source))
            .unwrap_or_default();
        println!("{}  {word:04X}  {}", item.address, text.trim_end());
    }
}
