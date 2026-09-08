//! A linter for MARIE assembly.
//!
//! The assembler already rejects everything that is not valid MARIE, so this crate is
//! about the programs that assemble perfectly and are still wrong. Most of those
//! mistakes are invisible to a single line and only show up once you know where control
//! can go, which is what [`cfg`] builds.
//!
//! # What it catches
//!
//! The three that matter most are all MARIE-specific:
//!
//! - [`falls-into-data`](lints::flow::FallsIntoData) — a missing `Halt` between the last
//!   instruction and the variables below it. `DEC 21` reached by the program counter
//!   decodes as `JnS 015`.
//! - [`masked-skipcond`](lints::hazards::MaskedSkipcond) — only bits 11-10 of a
//!   `Skipcond` operand select the condition, so `Skipcond 100` is not an error and not
//!   a fifth condition: it silently tests `AC < 0`.
//! - [`jns-overwrites-code`](lints::hazards::JnsOverwritesCode) — `JnS X` writes the
//!   return address into `M[X]`, so a call pointed at code destroys an instruction.
//!
//! # Levels
//!
//! Every lint has a [`Level`] that a caller can change: `Allow` silences it, `Warn`
//! keeps its natural severity, and `Deny` promotes it to an error that fails the run.
//! This is the mechanism behind a project's lint configuration.
//!
//! ```
//! use mrs_lint::{Level, Linter, lints::style::NON_CANONICAL_MNEMONIC};
//!
//! let source = "        Load  X\n        HALT\nX,      DEC 1\n";
//!
//! let linter = Linter::new()
//!     .allow(NON_CANONICAL_MNEMONIC)
//!     .set(mrs_lint::lints::flow::FALLS_OFF_END, Level::Deny);
//!
//! let outcome = linter.check(source);
//! assert!(outcome.diagnostics.iter().all(|d| d.code != NON_CANONICAL_MNEMONIC));
//! ```

use std::collections::BTreeMap;

use mrs_asm::diagnostic::{Code, Diagnostic, Severity};
use mrs_asm::lint::{Lint, LintContext};
use mrs_asm::{Assembly, assemble_into};

pub mod cfg;
pub mod lints;

use cfg::Cfg;

/// The namespace every code in this crate lives in.
pub const LINT: &str = "lint";

/// What to do with a lint's diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Level {
    /// Drop them.
    Allow,
    /// Report them at their natural severity.
    #[default]
    Warn,
    /// Report them as errors, failing the run.
    Deny,
}

/// Every lint this crate ships, in reporting order.
///
/// The flow lints come first because they are the ones most likely to be a real bug.
pub const ALL: &[&dyn Lint] = &[
    &lints::flow::FallsIntoData,
    &lints::flow::FallsOffEnd,
    &lints::flow::JumpsOutsideProgram,
    &lints::flow::UnreachableInstruction,
    &lints::hazards::MaskedSkipcond,
    &lints::hazards::SkipcondLabelOperand,
    &lints::hazards::JnsOverwritesCode,
    &lints::hazards::SelfModifyingCode,
    &lints::style::MissingHalt,
    &lints::style::NonCanonicalMnemonic,
    &lints::style::LabelShadowsMnemonic,
];

/// A configured linter.
///
/// Holds the lint set and the level for each code. [`Linter::new`] starts from every
/// lint this crate ships plus the assembler's own, all at [`Level::Warn`].
pub struct Linter<'a> {
    lints: Vec<&'a dyn Lint>,
    levels: BTreeMap<Code, Level>,
    default_level: Level,
}

impl Default for Linter<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Linter<'a> {
    /// A linter running every lint from this crate and from `mrs-asm`.
    pub fn new() -> Self {
        let mut lints = Vec::with_capacity(ALL.len() + mrs_asm::lint::STANDARD.len());
        lints.extend_from_slice(ALL);
        // The assembler's own lints cover the quirks of assembly rather than of
        // programs, so they belong in the default set too.
        lints.extend_from_slice(mrs_asm::lint::STANDARD);
        Self {
            lints,
            levels: BTreeMap::new(),
            default_level: Level::Warn,
        }
    }

    /// A linter with no lints registered, to be filled in by [`Linter::with`].
    pub fn empty() -> Self {
        Self {
            lints: Vec::new(),
            levels: BTreeMap::new(),
            default_level: Level::Warn,
        }
    }

    /// Registers an additional lint, which may come from any crate.
    #[must_use]
    pub fn with(mut self, lint: &'a dyn Lint) -> Self {
        self.lints.push(lint);
        self
    }

    /// Sets the level for one code.
    #[must_use]
    pub fn set(mut self, code: Code, level: Level) -> Self {
        self.levels.insert(code, level);
        self
    }

    /// Silences one code.
    #[must_use]
    pub fn allow(self, code: Code) -> Self {
        self.set(code, Level::Allow)
    }

    /// Promotes one code to an error.
    #[must_use]
    pub fn deny(self, code: Code) -> Self {
        self.set(code, Level::Deny)
    }

    /// Sets the level applied to codes that have no explicit level.
    #[must_use]
    pub fn default_level(mut self, level: Level) -> Self {
        self.default_level = level;
        self
    }

    /// Returns the level configured for `code`.
    pub fn level_of(&self, code: Code) -> Level {
        self.levels
            .get(&code)
            .copied()
            .unwrap_or(self.default_level)
    }

    /// Returns the registered lints, for listing them in help output.
    pub fn lints(&self) -> &[&'a dyn Lint] {
        &self.lints
    }

    /// Assembles `source` and runs the configured lints over it.
    pub fn check(&self, source: &str) -> Outcome {
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        // Assembler errors are not lints and are never re-levelled: a file that does not
        // assemble does not assemble.
        let mut assembly = assemble_into(source, &mut diagnostics);
        // `assembly.diagnostics` keeps the assembler's findings *only*, so
        // `Assembly::succeeded` — and therefore `Outcome::assembled` — still answers
        // "did this file assemble?" even when a lint has been promoted to an error.
        assembly.diagnostics = diagnostics.clone();

        let cx = LintContext {
            assembly: &assembly,
            source,
        };
        for lint in &self.lints {
            let level = self.level_of(lint.code());
            if level == Level::Allow {
                continue;
            }
            let mut produced: Vec<Diagnostic> = Vec::new();
            lint.run(&cx, &mut produced);
            for mut diagnostic in produced {
                if level == Level::Deny {
                    diagnostic.severity = Severity::Error;
                }
                diagnostics.push(diagnostic);
            }
        }

        Outcome {
            assembly,
            diagnostics,
        }
    }
}

/// The result of linting a file.
pub struct Outcome {
    /// The assembly, whether or not it succeeded.
    pub assembly: Assembly,
    /// Assembler diagnostics followed by lint diagnostics, with levels applied.
    pub diagnostics: Vec<Diagnostic>,
}

impl Outcome {
    /// Returns `true` if anything was reported as an error.
    ///
    /// A denied lint counts, which is what makes `Deny` fail a build.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// Returns `true` if the file assembled, ignoring lint levels.
    pub fn assembled(&self) -> bool {
        self.assembly.succeeded()
    }

    /// Iterates over the diagnostics reported as errors.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(|d| d.is_error())
    }

    /// Iterates over the diagnostics reported as warnings.
    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
    }

    /// Counts the diagnostics carrying `code`.
    pub fn count(&self, code: Code) -> usize {
        self.diagnostics.iter().filter(|d| d.code == code).count()
    }

    /// Returns `true` if any diagnostic carries `code`.
    pub fn has(&self, code: Code) -> bool {
        self.count(code) > 0
    }

    /// Builds a compiler-style report over every diagnostic.
    #[cfg(feature = "pretty")]
    pub fn report(&self, name: impl Into<String>, source: &str) -> mrs_asm::report::Report {
        mrs_asm::report::Report::new(name, source, &self.diagnostics)
    }
}

/// Builds the control-flow graph for a lint, or `None` if the program is not worth
/// analysing.
///
/// A file with assembly errors emits a zero word for every item it could not build, and
/// tracing control through those would report nonsense on top of real errors.
pub fn graph(cx: &LintContext<'_>) -> Option<Cfg> {
    if !cx.assembly.succeeded() || cx.assembly.program.items.is_empty() {
        return None;
    }
    Some(Cfg::new(cx.assembly))
}
