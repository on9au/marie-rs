//! The MARIE-rs assembler.
//!
//! Assembles [MARIE.js]-compatible assembly into machine words, and exposes everything
//! it learned on the way so that a linter, a formatter or a language server can reuse
//! the same front end rather than writing a second, subtly different one.
//!
//! # Compatibility
//!
//! This implements the MARIE.js assembler, not the textbook one, and it is deliberately
//! bug-compatible with it. The rules that are easiest to get wrong:
//!
//! - **Operands are hexadecimal.** `Add 123` assembles `0x3123`, not `0x307B`.
//! - **A leading digit makes an operand a literal.** `Load 1A` is address `0x1A`;
//!   `Load A1` is a reference to a label named `A1`.
//! - **Labels are case-sensitive; mnemonics are not.** `Foo` and `foo` are two labels,
//!   but `halt`, `Halt` and `HALT` are one instruction.
//! - **`ORG` takes exactly three hex digits**, must come before any word, and may
//!   appear once. `ORG 10` is not an origin directive at all — it is parsed as a
//!   statement and reported as an unknown mnemonic.
//! - **`END` stops assembly**, and nothing after it is even parsed.
//! - **A line with an invalid label is skipped whole**, shifting the addresses of
//!   everything after it.
//! - **Comments start with `/`** and run to the end of the line.
//!
//! # Tooling
//!
//! Every syntactic element carries a [`Span`](span::Span). [`Assembly`] hands back the
//! syntax tree, the [`SymbolTable`] with definition and reference sites, a two-way
//! [`SourceMap`], and a [`LineIndex`](span::LineIndex) for converting offsets to
//! line/column positions. Assembly never stops at the first error and never panics on
//! malformed input, so an editor can call it on every keystroke.
//!
//! # Example
//!
//! ```
//! use mrs_asm::assemble;
//!
//! let assembly = assemble("
//!     Load  X      / read the value
//!     Add   Y
//!     Store Z
//!     Halt
//! X,  DEC 5
//! Y,  DEC 37
//! Z,  DEC 0
//! ");
//!
//! assert!(assembly.succeeded());
//! assert_eq!(assembly.words[0], 0x1004_u16 as i16);
//! assert_eq!(assembly.symbols.address_of("X").unwrap().value(), 0x004);
//! ```
//!
//! [MARIE.js]: https://marie.js.org

pub mod assembler;
pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod lint;
pub mod parser;
#[cfg(feature = "pretty")]
pub mod report;
pub mod source_map;
pub mod span;
pub mod symbols;

use std::fmt;

use mrs_core::{MEMORY_WORD_COUNT, MemoryAddress, MemoryImage};

pub use ast::Program;
pub use diagnostic::{Code, Diagnostic, Severity, Sink};
pub use lint::Lint;
pub use source_map::SourceMap;
pub use span::{LineIndex, Position, Span};
pub use symbols::{Role, Symbol, SymbolTable};

/// How to assemble.
///
/// The lint set is a slice of trait objects rather than a flag, so a downstream tool
/// can mix its own [`Lint`]s in with the built-in ones, or run only its own.
#[derive(Clone, Copy)]
pub struct Options<'a> {
    /// The lints to run after assembly.
    ///
    /// Empty by default, because MARIE.js has no warnings and a compatibility check
    /// should see exactly what it sees. Lints never affect [`Assembly::succeeded`].
    pub lints: &'a [&'a dyn Lint],
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self::bare()
    }
}

impl fmt::Debug for Options<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Options")
            .field(
                "lints",
                &self.lints.iter().map(|l| l.code()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl<'a> Options<'a> {
    /// No lints: exactly what MARIE.js reports.
    pub const fn bare() -> Self {
        Self { lints: &[] }
    }

    /// The lints this crate ships.
    pub const fn linting() -> Self {
        Self {
            lints: lint::STANDARD,
        }
    }

    /// A specific set of lints, built-in or not.
    pub const fn with_lints(lints: &'a [&'a dyn Lint]) -> Self {
        Self { lints }
    }
}

/// The result of assembling a source file.
///
/// Produced whether or not assembly succeeded; check [`Assembly::succeeded`] before
/// using [`Assembly::words`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembly {
    /// The syntax tree, retained for tooling.
    pub program: Program,
    /// The address the program loads at, from `ORG` or `0x000`.
    pub origin: MemoryAddress,
    /// One word per statement, in source order. Statements that failed to assemble
    /// contribute a zero, so this stays aligned with `program.items`.
    pub words: Vec<i16>,
    /// Every label, with its definition and reference sites.
    pub symbols: SymbolTable,
    /// The address-to-line mapping, for debuggers.
    pub source_map: SourceMap,
    /// Everything that went wrong, parse problems first, then assembly problems.
    pub diagnostics: Vec<Diagnostic>,
    /// The line index for the source that produced this.
    pub lines: LineIndex,
}

impl Assembly {
    /// Returns `true` if nothing blocked assembly. Warnings do not count.
    pub fn succeeded(&self) -> bool {
        !self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// Iterates over the error diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(|d| d.is_error())
    }

    /// Iterates over the warning diagnostics.
    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
    }

    /// Returns the diagnostics ordered by source position.
    ///
    /// The `diagnostics` field keeps MARIE.js's two-pass order; an editor usually wants
    /// them sorted instead.
    pub fn diagnostics_in_source_order(&self) -> Vec<&Diagnostic> {
        let mut sorted: Vec<_> = self.diagnostics.iter().collect();
        sorted.sort_by_key(|d| (d.span.start, d.span.end, d.code));
        sorted
    }

    /// Builds a full memory image with the program loaded at its origin.
    ///
    /// Returns `None` if assembly failed.
    ///
    /// A program can never overflow memory by the time it gets here: the parser stops
    /// assigning addresses at the top of the address space and reports
    /// [`Code::ProgramTooLarge`]. That is the one place this assembler is stricter than
    /// MARIE.js, which assigns out-of-range addresses happily and only refuses the
    /// program when the simulator tries to load it. No runnable program is rejected by
    /// the difference — MARIE.js cannot load those either — so the error is raised at
    /// the point where it can name the offending line. The bounds check below is kept
    /// as a guard on the slice, not as a second diagnostic.
    pub fn image(&self) -> Option<MemoryImage> {
        let start = self.origin.index();
        let end = start.checked_add(self.words.len())?;
        if !self.succeeded() || end > MEMORY_WORD_COUNT as usize {
            return None;
        }
        let mut image = [0i16; MEMORY_WORD_COUNT as usize];
        image[start..end].copy_from_slice(&self.words);
        Some(image)
    }

    /// Returns the origin and words, ready for
    /// `MarieVM::load_program`.
    pub fn program_image(&self) -> (MemoryAddress, &[i16]) {
        (self.origin, &self.words)
    }
}

/// Assembles `source`, running no lints.
pub fn assemble(source: &str) -> Assembly {
    assemble_with(source, Options::bare())
}

/// Assembles `source`.
pub fn assemble_with(source: &str, options: Options<'_>) -> Assembly {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut assembly = assemble_into(source, &mut diagnostics);

    // Lints run against the finished assembly, so they see resolved addresses and
    // reference counts. Their diagnostics are appended after the assembler's own.
    if !options.lints.is_empty() {
        lint::run(options.lints, &assembly, source, &mut diagnostics);
    }
    assembly.diagnostics = diagnostics;
    assembly
}

/// Assembles `source`, streaming diagnostics to `sink` rather than collecting them.
///
/// The returned [`Assembly`] has an empty `diagnostics` field, and lints are not run —
/// they need a finished assembly, so use [`assemble_with`] for those. This is the entry
/// point for a caller that converts diagnostics as they arrive, caps how many it keeps,
/// or applies an allow-list with [`Filtered`](diagnostic::Filtered).
pub fn assemble_into(source: &str, sink: &mut dyn Sink) -> Assembly {
    let parsed = parser::parse(source, sink);
    let mut symbols = parsed.symbols;
    let words = assembler::assemble_program(&parsed.program, &mut symbols, sink);

    Assembly {
        origin: parsed.program.origin_address(),
        program: parsed.program,
        words,
        symbols,
        source_map: parsed.source_map,
        diagnostics: Vec::new(),
        lines: LineIndex::new(source),
    }
}
