//! Diagnostics produced by the assembler, and the vocabulary other tools extend.
//!
//! The assembler never stops at the first problem: it reports everything it finds and
//! still returns whatever it managed to build. That is what a language server needs —
//! source is broken most of the time it is looked at — and it is also what MARIE.js
//! does, which collects an error list across both of its passes.
//!
//! # Extensibility
//!
//! A [`Code`] is a namespace and a name rather than a closed enum, so a linter, a
//! language server or any other downstream crate can mint its own codes and push them
//! through the same [`Diagnostic`] type, the same [`Sink`], and the same renderer:
//!
//! ```
//! use mrs_asm::diagnostic::{Code, Diagnostic, Severity};
//! use mrs_asm::span::Span;
//!
//! // A code owned by some other crate.
//! const STYLE: &str = "housestyle";
//! const SHOUTY: Code = Code::new(STYLE, "shouty-mnemonic");
//!
//! let diagnostic = Diagnostic::new(Severity::Warning, SHOUTY, Span::new(0, 4), 0, "Avoid SHOUTING.")
//!     .with_help("Write `Halt` rather than `HALT`.");
//!
//! assert_eq!(diagnostic.code.to_string(), "housestyle::shouty-mnemonic");
//! ```
//!
//! The codes this crate emits all live in the [`ASM`] namespace and are exposed as
//! associated constants on [`Code`], so matching on them stays readable:
//!
//! ```
//! # use mrs_asm::{assemble, diagnostic::Code};
//! let assembly = assemble("Load Nope");
//! match assembly.errors().next().unwrap().code {
//!     Code::UNKNOWN_LABEL => {}
//!     other => panic!("unexpected {other}"),
//! }
//! ```

use std::fmt;

use crate::span::Span;

/// The namespace of every diagnostic this crate emits.
pub const ASM: &str = "asm";

/// How much a diagnostic matters.
///
/// Ordered most severe first, so sorting a diagnostic list surfaces errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The program cannot be assembled.
    Error,
    /// The program assembles, but something looks wrong.
    ///
    /// Warnings never affect [`Assembly::succeeded`](crate::Assembly::succeeded).
    Warning,
    /// A suggestion. Maps to an LSP hint.
    Advice,
}

impl Severity {
    /// Returns `true` if a diagnostic at this severity blocks assembly.
    pub const fn is_fatal(self) -> bool {
        matches!(self, Severity::Error)
    }

    /// Returns the severity's lowercase name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Advice => "advice",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stable, namespaced identifier for a kind of diagnostic.
///
/// Codes are stable across releases and are what a user should be able to put in an
/// allow-list; the human-readable message is not stable. Rendered as
/// `namespace::name`, the way a Clippy lint name reads.
///
/// Downstream crates construct their own with [`Code::new`] under their own namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Code {
    namespace: &'static str,
    name: &'static str,
}

impl Code {
    /// Creates a code in `namespace`.
    ///
    /// Use a namespace that identifies your tool; [`ASM`] is reserved for this crate.
    pub const fn new(namespace: &'static str, name: &'static str) -> Self {
        Self { namespace, name }
    }

    /// The namespace, e.g. `"asm"`.
    pub const fn namespace(self) -> &'static str {
        self.namespace
    }

    /// The bare name, e.g. `"unknown-label"`.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Returns `true` if this code was minted by this crate.
    pub fn is_builtin(self) -> bool {
        self.namespace == ASM
    }
}

/// The codes this crate emits.
///
/// The first group blocks assembly; the second is only produced by
/// [lints](crate::lint) and never does.
impl Code {
    /// The line does not fit the `label, operator operand / comment` shape.
    pub const MALFORMED_LINE: Self = Self::new(ASM, "malformed-line");
    /// An `ORG` appeared after an instruction, or a second time.
    pub const UNEXPECTED_ORIGIN: Self = Self::new(ASM, "unexpected-origin");
    /// A label began with a digit.
    pub const LABEL_STARTS_WITH_DIGIT: Self = Self::new(ASM, "label-starts-with-digit");
    /// A label contained whitespace.
    pub const LABEL_CONTAINS_WHITESPACE: Self = Self::new(ASM, "label-contains-whitespace");
    /// A label was defined twice.
    pub const DUPLICATE_LABEL: Self = Self::new(ASM, "duplicate-label");
    /// An operand named a label that was never defined.
    pub const UNKNOWN_LABEL: Self = Self::new(ASM, "unknown-label");
    /// The mnemonic is neither an instruction nor a directive.
    pub const UNKNOWN_MNEMONIC: Self = Self::new(ASM, "unknown-mnemonic");
    /// An instruction or directive that needs an operand did not get one.
    pub const MISSING_OPERAND: Self = Self::new(ASM, "missing-operand");
    /// An operand was given to something that takes none.
    pub const UNEXPECTED_OPERAND: Self = Self::new(ASM, "unexpected-operand");
    /// A `DEC`/`OCT`/`HEX` literal could not be parsed in its base.
    pub const MALFORMED_LITERAL: Self = Self::new(ASM, "malformed-literal");
    /// A literal does not fit in a 16-bit word.
    pub const LITERAL_OUT_OF_RANGE: Self = Self::new(ASM, "literal-out-of-range");
    /// A literal operand does not fit in the 12-bit address field.
    pub const ADDRESS_OUT_OF_RANGE: Self = Self::new(ASM, "address-out-of-range");
    /// The program has more words than the address space holds.
    pub const PROGRAM_TOO_LARGE: Self = Self::new(ASM, "program-too-large");

    /// An operand was silently discarded by the assembler. Lint only.
    pub const IGNORED_OPERAND: Self = Self::new(ASM, "ignored-operand");
    /// A label is defined but never referenced. Lint only.
    pub const UNUSED_LABEL: Self = Self::new(ASM, "unused-label");
    /// Code follows an `END` directive and will never be assembled. Lint only.
    pub const UNREACHABLE_CODE: Self = Self::new(ASM, "unreachable-code");

    /// Every code this crate emits, for building allow-lists and documentation.
    pub const ALL: [Self; 16] = [
        Self::MALFORMED_LINE,
        Self::UNEXPECTED_ORIGIN,
        Self::LABEL_STARTS_WITH_DIGIT,
        Self::LABEL_CONTAINS_WHITESPACE,
        Self::DUPLICATE_LABEL,
        Self::UNKNOWN_LABEL,
        Self::UNKNOWN_MNEMONIC,
        Self::MISSING_OPERAND,
        Self::UNEXPECTED_OPERAND,
        Self::MALFORMED_LITERAL,
        Self::LITERAL_OUT_OF_RANGE,
        Self::ADDRESS_OUT_OF_RANGE,
        Self::PROGRAM_TOO_LARGE,
        Self::IGNORED_OPERAND,
        Self::UNUSED_LABEL,
        Self::UNREACHABLE_CODE,
    ];
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.namespace, self.name)
    }
}

/// A span with an optional note, rendered as an underline under the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// Where the label points.
    pub span: Span,
    /// What it says, if anything.
    pub message: Option<String>,
}

impl Label {
    /// Creates a label carrying a note.
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: Some(message.into()),
        }
    }

    /// Creates a bare underline with no note.
    pub const fn bare(span: Span) -> Self {
        Self {
            span,
            message: None,
        }
    }
}

/// A problem found while assembling, or by a lint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// How much it matters.
    pub severity: Severity,
    /// The stable identifier for this kind of problem.
    pub code: Code,
    /// The primary location: what an editor underlines and jumps to.
    pub span: Span,
    /// Zero-based line the span starts on, cached so callers can group by line without
    /// a [`LineIndex`](crate::span::LineIndex).
    pub line: u32,
    /// The human-readable description.
    pub message: String,
    /// Secondary underlines, such as the first definition of a duplicated label.
    pub labels: Vec<Label>,
    /// An optional suggestion, rendered as a compiler's trailing `help:` line.
    pub help: Option<String>,
}

impl Diagnostic {
    /// Builds a diagnostic at an explicit severity.
    pub fn new(
        severity: Severity,
        code: Code,
        span: Span,
        line: u32,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code,
            span,
            line,
            message: message.into(),
            labels: Vec::new(),
            help: None,
        }
    }

    /// Builds an error.
    pub fn error(code: Code, span: Span, line: u32, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, span, line, message)
    }

    /// Builds a warning.
    pub fn warning(code: Code, span: Span, line: u32, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, span, line, message)
    }

    /// Builds a piece of advice.
    pub fn advice(code: Code, span: Span, line: u32, message: impl Into<String>) -> Self {
        Self::new(Severity::Advice, code, span, line, message)
    }

    /// Attaches a secondary underline.
    #[must_use]
    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label::new(span, message));
        self
    }

    /// Attaches a trailing suggestion.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Returns `true` if this diagnostic prevents assembly.
    pub fn is_error(&self) -> bool {
        self.severity.is_fatal()
    }
}

impl fmt::Display for Diagnostic {
    /// Formats as `error[asm::code] at line N: message`, with one-based line numbers.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[{}] at line {}: {}",
            self.severity,
            self.code,
            self.line + 1,
            self.message
        )
    }
}

/// Somewhere diagnostics can be sent.
///
/// Lints are written against this rather than against `Vec<Diagnostic>` so that a
/// caller can filter, count, cap or forward diagnostics as they are produced — an LSP
/// converting straight to protocol types never has to build the intermediate vector.
pub trait Sink {
    /// Accepts a diagnostic.
    fn push(&mut self, diagnostic: Diagnostic);
}

impl Sink for Vec<Diagnostic> {
    fn push(&mut self, diagnostic: Diagnostic) {
        Vec::push(self, diagnostic);
    }
}

impl<S: Sink + ?Sized> Sink for &mut S {
    fn push(&mut self, diagnostic: Diagnostic) {
        (**self).push(diagnostic);
    }
}

/// Adapts a closure into a [`Sink`].
///
/// A blanket `impl<F: FnMut(Diagnostic)> Sink for F` would overlap the forwarding impl
/// above, because `&mut F` is itself `FnMut`, so the closure case is opt-in.
#[derive(Debug, Clone, Copy)]
pub struct FromFn<F>(pub F);

impl<F: FnMut(Diagnostic)> Sink for FromFn<F> {
    fn push(&mut self, diagnostic: Diagnostic) {
        (self.0)(diagnostic);
    }
}

/// A [`Sink`] that drops everything, for a caller that only wants the words.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ignore;

impl Sink for Ignore {
    fn push(&mut self, _diagnostic: Diagnostic) {}
}

/// A [`Sink`] that keeps only diagnostics whose code passes a filter.
///
/// This is how an allow-list is applied: wrap the destination and drop the codes the
/// user has silenced.
#[derive(Debug)]
pub struct Filtered<S, F> {
    inner: S,
    predicate: F,
}

impl<S: Sink, F: FnMut(&Diagnostic) -> bool> Filtered<S, F> {
    /// Wraps `inner`, forwarding only diagnostics for which `predicate` returns `true`.
    pub fn new(inner: S, predicate: F) -> Self {
        Self { inner, predicate }
    }

    /// Unwraps, returning the destination.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S: Sink, F: FnMut(&Diagnostic) -> bool> Sink for Filtered<S, F> {
    fn push(&mut self, diagnostic: Diagnostic) {
        if (self.predicate)(&diagnostic) {
            self.inner.push(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_codes_render_in_their_namespace() {
        assert_eq!(Code::UNKNOWN_LABEL.to_string(), "asm::unknown-label");
        assert_eq!(Code::UNKNOWN_LABEL.name(), "unknown-label");
        assert!(Code::UNKNOWN_LABEL.is_builtin());
    }

    #[test]
    fn a_foreign_code_is_distinguishable_from_a_built_in_one() {
        let mine = Code::new("mylint", "unknown-label");
        assert!(!mine.is_builtin());
        // Same name, different namespace, so they never collide.
        assert_ne!(mine, Code::UNKNOWN_LABEL);
        assert_eq!(mine.to_string(), "mylint::unknown-label");
    }

    #[test]
    fn built_in_codes_are_usable_as_match_patterns() {
        // This is the property that keeps an open code type as ergonomic as an enum.
        let code = Code::UNKNOWN_LABEL;
        let matched = match code {
            Code::UNKNOWN_LABEL => "label",
            Code::MISSING_OPERAND => "operand",
            _ => "other",
        };
        assert_eq!(matched, "label");
    }

    #[test]
    fn every_built_in_code_is_unique() {
        let mut names: Vec<_> = Code::ALL.iter().map(|c| c.name()).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total, "duplicate diagnostic code");
    }

    #[test]
    fn diagnostics_render_with_one_based_lines() {
        let diagnostic =
            Diagnostic::error(Code::UNKNOWN_LABEL, Span::new(4, 7), 2, "Unknown label.");
        assert_eq!(
            diagnostic.to_string(),
            "error[asm::unknown-label] at line 3: Unknown label."
        );
        assert!(diagnostic.is_error());
        assert!(!Diagnostic::warning(Code::UNUSED_LABEL, Span::default(), 0, "x").is_error());
    }

    #[test]
    fn a_closure_and_a_vec_are_both_sinks() {
        let mut collected = Vec::new();
        {
            let mut sink = FromFn(|d: Diagnostic| collected.push(d.code));
            sink.push(Diagnostic::error(
                Code::UNKNOWN_LABEL,
                Span::default(),
                0,
                "a",
            ));
        }
        assert_eq!(collected, vec![Code::UNKNOWN_LABEL]);

        let mut vector: Vec<Diagnostic> = Vec::new();
        Sink::push(
            &mut vector,
            Diagnostic::error(Code::MISSING_OPERAND, Span::default(), 0, "b"),
        );
        assert_eq!(vector.len(), 1);
    }

    #[test]
    fn a_filtered_sink_applies_an_allow_list() {
        let mut kept: Vec<Diagnostic> = Vec::new();
        {
            let mut sink = Filtered::new(&mut kept, |d: &Diagnostic| d.code != Code::UNUSED_LABEL);
            sink.push(Diagnostic::warning(
                Code::UNUSED_LABEL,
                Span::default(),
                0,
                "silenced",
            ));
            sink.push(Diagnostic::error(
                Code::UNKNOWN_LABEL,
                Span::default(),
                0,
                "kept",
            ));
        }
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].code, Code::UNKNOWN_LABEL);
    }
}
