//! Publishing assembler errors and lint findings.

use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString, Uri,
};
use mrs_asm::Severity;

use crate::document::Document;
use crate::encoding::PositionEncoding;

/// Converts every finding in `document` into an LSP diagnostic.
pub fn diagnostics(document: &Document, uri: &Uri, encoding: PositionEncoding) -> Vec<Diagnostic> {
    let positions = document.positions(encoding);
    document
        .outcome
        .diagnostics
        .iter()
        .map(|finding| {
            let related: Vec<_> = finding
                .labels
                .iter()
                .map(|label| DiagnosticRelatedInformation {
                    location: Location::new(uri.clone(), positions.range(label.span)),
                    message: label
                        .message
                        .clone()
                        .unwrap_or_else(|| "related".to_owned()),
                })
                .collect();

            Diagnostic {
                range: positions.range(finding.span),
                severity: Some(severity(finding.severity)),
                // The stable code is what an editor shows and what a user silences by.
                code: Some(NumberOrString::String(finding.code.to_string())),
                code_description: None,
                source: Some(finding.code.namespace().to_owned()),
                // The trailing `help:` line belongs with the message in an editor,
                // which has no separate place to show it.
                message: match &finding.help {
                    Some(help) => format!("{}\n\nhelp: {help}", finding.message),
                    None => finding.message.clone(),
                },
                related_information: (!related.is_empty()).then_some(related),
                tags: None,
                data: None,
            }
        })
        .collect()
}

/// Maps this workspace's severities onto the protocol's.
fn severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        // Advice is guidance rather than a problem, which is what a hint is for.
        Severity::Advice => DiagnosticSeverity::HINT,
    }
}
