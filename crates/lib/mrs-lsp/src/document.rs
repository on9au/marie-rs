//! An open document and its analysis.
//!
//! A language server is asked about a file many times between edits — hover, then
//! tokens, then hints, then code actions — so the analysis is computed once when the
//! text changes and every feature reads the cached result.

use mrs_asm::Assembly;
use mrs_asm::span::LineIndex;
use mrs_lint::{Linter, Outcome};

use crate::encoding::{PositionEncoding, Positions};

/// An open document, together with everything derived from its text.
pub struct Document {
    /// The current text.
    pub text: String,
    /// The version the client last sent.
    pub version: i32,
    /// The assembly and lint findings.
    pub outcome: Outcome,
}

impl Document {
    /// Analyses `text`.
    pub fn new(text: String, version: i32, linter: &Linter<'_>) -> Self {
        let outcome = linter.check(&text);
        Self {
            text,
            version,
            outcome,
        }
    }

    /// Replaces the text and re-analyses.
    pub fn update(&mut self, text: String, version: i32, linter: &Linter<'_>) {
        self.outcome = linter.check(&text);
        self.text = text;
        self.version = version;
    }

    /// The assembled program.
    pub fn assembly(&self) -> &Assembly {
        &self.outcome.assembly
    }

    /// The line index for this document.
    pub fn lines(&self) -> &LineIndex {
        &self.outcome.assembly.lines
    }

    /// A position converter for this document.
    pub fn positions(&self, encoding: PositionEncoding) -> Positions<'_> {
        Positions::new(&self.text, self.lines(), encoding)
    }
}
