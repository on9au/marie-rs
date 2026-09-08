//! Loading a program from either source or a binary image.
//!
//! `run` and `debug` accept both, so the decision of which one a file is lives here
//! rather than being made twice.

use std::path::Path;

use clap::{Args as ClapArgs, ValueEnum};
use mrs_asm::assemble;
use mrs_core::{MemoryAddress, image};

use crate::{name, read};

/// How to interpret an input file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Format {
    /// Decide from the file extension: `.bin` is an image, anything else is source.
    #[default]
    Auto,
    /// MARIE assembly.
    Asm,
    /// A little-endian 16-bit memory image, as MARIE.js writes.
    Bin,
}

impl Format {
    /// Resolves [`Format::Auto`] against a path.
    fn resolve(self, path: &Path) -> Format {
        match self {
            Format::Auto => {
                let is_bin = path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("bin"));
                if is_bin { Format::Bin } else { Format::Asm }
            }
            explicit => explicit,
        }
    }
}

/// The flags shared by every subcommand that loads a program.
#[derive(Debug, ClapArgs)]
pub struct Load {
    /// How to read the file. By default `.bin` is an image and anything else is source.
    #[arg(long, value_enum, default_value_t = Format::Auto)]
    pub format: Format,

    /// Load the program at this hexadecimal address.
    ///
    /// Ignored for a full memory image, which already covers the whole address space,
    /// and for assembly with an `ORG` directive.
    #[arg(long, value_name = "ADDR")]
    pub origin: Option<String>,
}

/// A program ready to be put into a machine.
///
/// Where the words *live* and where execution *starts* are separate: a full memory
/// image occupies the whole address space and always loads at zero, but `--origin` can
/// still say which word to begin at. Conflating the two makes `--origin` unusable with
/// a full image, since 4096 words fit nowhere but address zero.
pub struct Program {
    /// The words to load.
    pub words: Vec<i16>,
    /// Where the words go.
    pub load_at: MemoryAddress,
    /// Where execution begins.
    pub entry: MemoryAddress,
    /// Whether the words cover the whole address space.
    pub full_image: bool,
    /// The assembly, when the input was source. `None` for an image, which carries no
    /// labels, no line numbers and no source map.
    pub assembly: Option<mrs_asm::Assembly>,
    /// The source text, when there was any.
    pub source: Option<String>,
}

impl Program {
    /// Returns `true` if this came from source and so has debugging information.
    pub fn has_source(&self) -> bool {
        self.assembly.is_some()
    }

    /// Loads the program into a machine and positions it at the entry point.
    pub fn install<IO: mrs_vm::io::MarieVmIODevice>(
        &self,
        vm: &mut mrs_vm::MarieVM<IO, mrs_vm::states::Ready>,
    ) -> miette::Result<()> {
        if self.full_image {
            // The words are the whole of memory, so they are flashed rather than
            // loaded, and the entry point is set independently.
            let mut image = [0i16; mrs_core::MEMORY_WORD_COUNT as usize];
            image.copy_from_slice(&self.words);
            vm.flash_program(&image, self.entry);
            return Ok(());
        }
        vm.load_program(self.load_at, &self.words)
            .map_err(|error| miette::miette!("{error}"))?;
        // `load_program` starts at the load address; an explicit entry overrides it.
        if self.entry != self.load_at {
            vm.registers_mut().pc = self.entry;
        }
        Ok(())
    }
}

/// Loads a program from `path`, assembling it if it is source.
pub fn load(path: &Path, options: &Load) -> miette::Result<Program> {
    let origin = match &options.origin {
        Some(written) => parse_origin(written)?,
        None => MemoryAddress::ZERO,
    };

    match options.format.resolve(path) {
        Format::Bin => load_image(path, origin),
        // `Auto` is resolved above, so only `Asm` reaches here.
        _ => load_source(path, options.origin.is_some().then_some(origin)),
    }
}

/// Reads a binary memory image.
fn load_image(path: &Path, origin: MemoryAddress) -> miette::Result<Program> {
    let bytes = std::fs::read(path)
        .map_err(|error| miette::miette!("could not read {}: {error}", path.display()))?;

    // A full-size file is a dump of the whole address space, so it loads at zero and
    // `--origin` only says where to start executing. Anything shorter is a fragment
    // that has to be placed somewhere.
    let full = image::is_full_image(&bytes);
    let words = image::decode_words(&bytes).map_err(|error| {
        miette::miette!(
            help = "MARIE.js writes 8192 bytes of little-endian 16-bit words",
            "{}: {error}",
            name(path)
        )
    })?;

    if !full && words.len() + origin.index() > mrs_core::MEMORY_WORD_COUNT as usize {
        return Err(miette::miette!(
            help = "lower --origin, or use a smaller image",
            "an image of {} words does not fit at origin {origin}",
            words.len()
        ));
    }

    Ok(Program {
        words,
        // A full dump is the whole address space and can only sit at zero; a fragment
        // goes wherever it was asked to.
        load_at: if full { MemoryAddress::ZERO } else { origin },
        entry: origin,
        full_image: full,
        assembly: None,
        source: None,
    })
}

/// Reads and assembles a source file.
fn load_source(path: &Path, origin: Option<MemoryAddress>) -> miette::Result<Program> {
    let source = read(path)?;
    let assembly = assemble(&source);
    if !assembly.succeeded() {
        return Err(assembly.report(name(path), &source).into());
    }
    // An explicit `--origin` would contradict whatever the source says, so it is only
    // honoured when the program did not choose for itself.
    if let Some(origin) = origin
        && assembly.program.origin.is_some()
        && assembly.origin != origin
    {
        return Err(miette::miette!(
            help = "remove --origin, or the ORG directive",
            "--origin {origin} conflicts with the ORG directive at {}",
            assembly.origin
        ));
    }

    Ok(Program {
        words: assembly.words.clone(),
        load_at: assembly.origin,
        entry: assembly.origin,
        full_image: false,
        assembly: Some(assembly),
        source: Some(source),
    })
}

/// Parses a hexadecimal origin written on the command line.
pub fn parse_origin(written: &str) -> miette::Result<MemoryAddress> {
    let trimmed = written.trim_start_matches("0x").trim_start_matches("0X");
    let value = u16::from_str_radix(trimmed, 16)
        .map_err(|_| miette::miette!("'{written}' is not a hexadecimal address"))?;
    MemoryAddress::try_new(value)
        .ok_or_else(|| miette::miette!("address {written} is outside the 12-bit address space"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_decides_the_format() {
        assert_eq!(Format::Auto.resolve(Path::new("a.bin")), Format::Bin);
        assert_eq!(Format::Auto.resolve(Path::new("a.BIN")), Format::Bin);
        assert_eq!(Format::Auto.resolve(Path::new("a.mas")), Format::Asm);
        assert_eq!(Format::Auto.resolve(Path::new("a")), Format::Asm);
        // An explicit choice always wins.
        assert_eq!(Format::Asm.resolve(Path::new("a.bin")), Format::Asm);
        assert_eq!(Format::Bin.resolve(Path::new("a.mas")), Format::Bin);
    }

    #[test]
    fn origins_are_hexadecimal_and_bounded() {
        assert_eq!(parse_origin("100").unwrap().value(), 0x100);
        assert_eq!(parse_origin("0x0ff").unwrap().value(), 0x0ff);
        assert!(parse_origin("1000").is_err(), "outside 12 bits");
        assert!(parse_origin("zz").is_err());
    }
}
