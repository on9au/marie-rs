//! Compiler-style rendering.
//!
//! Rendered with miette's unthemed graphical handler so the output is plain ASCII and
//! deterministic, rather than depending on terminal colour support.

#![cfg(feature = "pretty")]

use miette::{GraphicalReportHandler, GraphicalTheme};
use mrs_asm::report::Report;
use mrs_asm::{Options, assemble, assemble_with};

/// Renders a report to a string, without colour or Unicode box drawing.
fn render(report: &Report) -> String {
    let mut out = String::new();
    GraphicalReportHandler::new_themed(GraphicalTheme::none())
        .render_report(&mut out, report)
        .expect("rendering cannot fail");
    out
}

#[test]
fn a_report_quotes_the_source_and_underlines_the_span() {
    let source = "Load Nope\n";
    let assembly = assemble(source);
    let rendered = render(&assembly.report("add.mas", source));

    assert!(rendered.contains("add.mas"), "names the file:\n{rendered}");
    assert!(
        rendered.contains("Load Nope"),
        "quotes the line:\n{rendered}"
    );
    assert!(
        rendered.contains("Unknown label 'Nope'."),
        "states the problem:\n{rendered}"
    );
    assert!(
        rendered.contains("asm::unknown-label"),
        "names the code:\n{rendered}"
    );
    assert!(rendered.contains("help:"), "offers a fix:\n{rendered}");
}

#[test]
fn a_secondary_label_points_at_the_related_location() {
    let source = "Dup, DEC 1\nDup, DEC 2\n";
    let assembly = assemble(source);
    let rendered = render(&assembly.report("dup.mas", source));
    assert!(rendered.contains("Labels must be unique"), "{rendered}");
    // The note attached to the first definition is rendered as a second underline.
    assert!(rendered.contains("first defined here"), "{rendered}");
}

#[test]
fn warnings_render_at_warning_severity() {
    let source = "Spare, DEC 1\nHalt\n";
    let assembly = assemble_with(source, Options::linting());
    assert!(assembly.succeeded());
    let rendered = render(&assembly.report("w.mas", source));
    assert!(rendered.contains("asm::unused-label"), "{rendered}");
    assert!(rendered.contains("never used"), "{rendered}");
}

#[test]
fn error_report_is_none_when_assembly_succeeded() {
    let source = "Halt\n";
    let assembly = assemble(source);
    assert!(assembly.error_report("ok.mas", source).is_none());

    let broken = assemble("Load Nope\n");
    let report = broken
        .error_report("bad.mas", "Load Nope\n")
        .expect("errors");
    assert_eq!(report.len(), 1);
    assert!(!report.is_empty());
}

#[test]
fn error_report_omits_warnings() {
    // A file with both: only the error should reach an error-only report.
    let source = "Spare, DEC 1\nLoad Nope\n";
    let assembly = assemble_with(source, Options::linting());
    let report = assembly.error_report("mixed.mas", source).expect("errors");
    assert_eq!(report.len(), 1, "warnings must not be included");

    let full = assembly.report("mixed.mas", source);
    assert_eq!(full.len(), 2, "the full report keeps both");
}

#[test]
fn a_hex_looking_operand_gets_a_targeted_suggestion() {
    // The `Skipcond C00` trap: suggest the leading zero rather than generic advice.
    let source = "Skipcond C00\n";
    let assembly = assemble(source);
    let rendered = render(&assembly.report("skip.mas", source));
    assert!(
        rendered.contains("write it as `0C00`"),
        "should suggest the leading zero:\n{rendered}"
    );

    // A name that is not hex at all keeps the general advice.
    let other = "Load Nope\n";
    let rendered = render(&assemble(other).report("o.mas", other));
    assert!(rendered.contains("Define it as `label, ...`"), "{rendered}");
}

#[test]
fn a_report_survives_multi_byte_source() {
    // Spans are byte offsets; miette must not slice through a character.
    let source = "caf\u{e9}, Load Nope\n";
    let assembly = assemble(source);
    let rendered = render(&assembly.report("u.mas", source));
    assert!(rendered.contains("Unknown label 'Nope'."), "{rendered}");
}

#[test]
fn a_report_over_many_diagnostics_renders_every_one() {
    let source = "Load A\nLoad B\nLoad C\n";
    let assembly = assemble(source);
    let report = assembly.report("many.mas", source);
    assert_eq!(report.len(), 3);
    let rendered = render(&report);
    for name in ["'A'", "'B'", "'C'"] {
        assert!(rendered.contains(name), "missing {name}:\n{rendered}");
    }
}

#[test]
fn a_report_is_an_error_and_can_be_returned_from_main() {
    let source = "Load Nope\n";
    let assembly = assemble(source);
    let report = assembly.error_report("m.mas", source).unwrap();
    // Both bounds are what `fn main() -> miette::Result<()>` needs.
    let as_error: &dyn std::error::Error = &report;
    assert!(as_error.to_string().contains("could not assemble m.mas"));
    let _: miette::Report = report.into();
}
