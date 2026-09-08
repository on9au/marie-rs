//! Compiler-style rendering of diagnostics.
//!
//! Wraps an [`Assembly`]'s diagnostics into a [`miette`] report, so a CLI gets the
//! familiar rustc layout — source excerpt, carets under the offending span, secondary
//! labels, a trailing `help:` line — instead of a list of line numbers.
//!
//! ```text
//!   × could not assemble add.mas
//!   ╰─▶ asm::unknown-label
//!
//!    ╭─[add.mas:1:6]
//!  1 │ Load Nope
//!    ·      ──┬─
//!    ·        ╰── Unknown label 'Nope'.
//!    ╰────
//!   help: Define it as `label, ...`, or write a hexadecimal address
//!         starting with a digit.
//! ```
//!
//! Available with the `pretty` feature, which is on by default. Turn it off for an
//! embedder that converts diagnostics into its own protocol types — a language server
//! never needs a terminal renderer — and the rest of the crate is unaffected.

use std::sync::Arc;

use miette::{LabeledSpan, NamedSource, SourceCode};

use crate::diagnostic::{Diagnostic, Severity};
use crate::{Assembly, Span};

/// Converts one of our spans into miette's offset/length form.
fn labeled(span: Span, message: Option<&str>) -> LabeledSpan {
    LabeledSpan::new(
        message.map(str::to_owned),
        span.start as usize,
        span.len() as usize,
    )
}

/// The source text, shared by every entry in a report rather than cloned per entry.
#[derive(Debug, Clone)]
struct Shared(Arc<NamedSource<String>>);

impl SourceCode for Shared {
    fn read_span<'a>(
        &'a self,
        span: &miette::SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn miette::SpanContents<'a> + 'a>, miette::MietteError> {
        self.0
            .read_span(span, context_lines_before, context_lines_after)
    }
}

/// A single rendered diagnostic.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Entry {
    message: String,
    code: String,
    severity: miette::Severity,
    help: Option<String>,
    labels: Vec<LabeledSpan>,
    excerpt: Shared,
}

impl miette::Diagnostic for Entry {
    fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> {
        Some(Box::new(&self.code))
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(self.severity)
    }

    fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> {
        self.help
            .as_ref()
            .map(|help| Box::new(help) as Box<dyn std::fmt::Display>)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(self.labels.iter().cloned()))
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&self.excerpt)
    }
}

/// Every diagnostic from one source file, rendered as a single report.
///
/// Implements [`std::error::Error`] and [`miette::Diagnostic`], so it can be returned
/// from `main` or printed with `{:?}` under miette's `fancy` feature.
#[derive(Debug, thiserror::Error)]
#[error("{title}")]
pub struct Report {
    name: String,
    title: String,
    failed: bool,
    entries: Vec<Entry>,
}

impl miette::Diagnostic for Report {
    fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> {
        Some(Box::new(if self.failed {
            "asm::assembly-failed"
        } else {
            "asm::findings"
        }))
    }

    fn related(&self) -> Option<Box<dyn Iterator<Item = &dyn miette::Diagnostic> + '_>> {
        Some(Box::new(
            self.entries.iter().map(|e| e as &dyn miette::Diagnostic),
        ))
    }
}

impl Report {
    /// Builds a report over `diagnostics`, quoting `source` under the name `name`.
    pub fn new<'a>(
        name: impl Into<String>,
        source: &str,
        diagnostics: impl IntoIterator<Item = &'a Diagnostic>,
    ) -> Self {
        let name = name.into();
        let shared = Shared(Arc::new(NamedSource::new(&name, source.to_owned())));
        let diagnostics: Vec<&Diagnostic> = diagnostics.into_iter().collect();
        // A report over warnings alone must not claim the file failed to assemble.
        let errors = diagnostics.iter().filter(|d| d.is_error()).count();
        let title = if errors > 0 {
            format!("could not assemble {name}")
        } else {
            let count = diagnostics.len();
            let plural = if count == 1 { "" } else { "s" };
            format!("{count} finding{plural} in {name}")
        };
        let entries = diagnostics
            .into_iter()
            .map(|diagnostic| Entry {
                message: diagnostic.message.clone(),
                code: diagnostic.code.to_string(),
                severity: match diagnostic.severity {
                    Severity::Error => miette::Severity::Error,
                    Severity::Warning => miette::Severity::Warning,
                    Severity::Advice => miette::Severity::Advice,
                },
                help: diagnostic.help.clone(),
                labels: std::iter::once(labeled(diagnostic.span, Some(&diagnostic.message)))
                    .chain(
                        diagnostic
                            .labels
                            .iter()
                            .map(|label| labeled(label.span, label.message.as_deref())),
                    )
                    .collect(),
                excerpt: shared.clone(),
            })
            .collect();
        Self {
            name,
            title,
            failed: errors > 0,
            entries,
        }
    }

    /// Returns `true` if the report contains at least one error.
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// The name the source was reported under.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the number of diagnostics in the report.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the report is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Assembly {
    /// Builds a [`Report`] over every diagnostic, for printing.
    ///
    /// `name` is the file name shown in the excerpt header.
    pub fn report(&self, name: impl Into<String>, source: &str) -> Report {
        Report::new(name, source, &self.diagnostics)
    }

    /// Builds a [`Report`] over the errors only.
    ///
    /// Returns `None` if assembly succeeded, so this composes with `?` in a `main`
    /// that returns [`miette::Result`].
    pub fn error_report(&self, name: impl Into<String>, source: &str) -> Option<Report> {
        if self.succeeded() {
            return None;
        }
        Some(Report::new(name, source, self.errors()))
    }
}
