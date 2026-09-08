//! Lint levels, the registry, and third-party lints.

use mrs_asm::diagnostic::{Code, Diagnostic, Severity, Sink};
use mrs_asm::lint::{Lint, LintContext};
use mrs_lint::lints::flow::FALLS_INTO_DATA;
use mrs_lint::lints::style::{MISSING_HALT, NON_CANONICAL_MNEMONIC};
use mrs_lint::{ALL, Level, Linter};

/// A program that falls into its data and shouts its mnemonics.
const MESSY: &str = "        LOAD  X\n        ADD   X\nX,      DEC 21\n";

#[test]
fn warn_is_the_default_level() {
    let outcome = Linter::new().check(MESSY);
    assert!(outcome.has(FALLS_INTO_DATA));
    assert!(!outcome.has_errors(), "warnings are not errors");
    assert_eq!(Linter::new().level_of(FALLS_INTO_DATA), Level::Warn);
}

#[test]
fn allow_silences_a_code_entirely() {
    let outcome = Linter::new().allow(FALLS_INTO_DATA).check(MESSY);
    assert!(!outcome.has(FALLS_INTO_DATA));
    // Other lints keep running.
    assert!(outcome.has(NON_CANONICAL_MNEMONIC));
}

#[test]
fn deny_promotes_a_warning_to_an_error() {
    let outcome = Linter::new().deny(FALLS_INTO_DATA).check(MESSY);
    assert!(outcome.has_errors(), "a denied lint fails the run");
    let diagnostic = outcome
        .diagnostics
        .iter()
        .find(|d| d.code == FALLS_INTO_DATA)
        .unwrap();
    assert_eq!(diagnostic.severity, Severity::Error);
}

#[test]
fn denying_a_lint_does_not_claim_the_file_failed_to_assemble() {
    // The distinction matters: a caller that wants the words should still get them.
    let outcome = Linter::new().deny(FALLS_INTO_DATA).check(MESSY);
    assert!(outcome.has_errors(), "the run failed");
    assert!(outcome.assembled(), "but the file assembled fine");
    assert!(
        outcome.assembly.image().is_some(),
        "and the image is usable"
    );
}

#[test]
fn the_default_level_can_be_changed_wholesale() {
    // `--deny warnings`.
    let outcome = Linter::new().default_level(Level::Deny).check(MESSY);
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|d| d.severity == Severity::Error)
    );

    // `--allow warnings`: an explicit level still wins over the default.
    let outcome = Linter::new()
        .default_level(Level::Allow)
        .set(FALLS_INTO_DATA, Level::Warn)
        .check(MESSY);
    assert_eq!(
        outcome
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        vec![FALLS_INTO_DATA]
    );
}

#[test]
fn an_allowed_lint_does_not_run_at_all() {
    // Not just filtered afterwards: a lint that is allowed should cost nothing.
    struct Explodes;
    impl Lint for Explodes {
        fn code(&self) -> Code {
            Code::new("test", "explodes")
        }
        fn run(&self, _: &LintContext<'_>, _: &mut dyn Sink) {
            panic!("an allowed lint must not run");
        }
    }
    let lint = Explodes;
    let outcome = Linter::empty()
        .with(&lint)
        .allow(Code::new("test", "explodes"))
        .check("Halt\n");
    assert!(outcome.diagnostics.is_empty());
}

#[test]
fn a_third_party_lint_registers_alongside_the_built_in_ones() {
    const NO_INPUT: Code = Code::new("housestyle", "no-input");

    struct RequiresInput;
    impl Lint for RequiresInput {
        fn code(&self) -> Code {
            NO_INPUT
        }
        fn description(&self) -> &'static str {
            "the program never reads input"
        }
        fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
            let reads = cx
                .assembly
                .program
                .items
                .iter()
                .any(|i| i.mnemonic.opcode() == Some(mrs_core::Opcode::Input));
            if !reads && !cx.assembly.program.items.is_empty() {
                out.push(Diagnostic::warning(
                    self.code(),
                    cx.assembly.program.items[0].span,
                    0,
                    "This program never reads input.",
                ));
            }
        }
    }

    let lint = RequiresInput;
    let outcome = Linter::new().with(&lint).check("Halt\n");
    assert!(outcome.has(NO_INPUT), "{:#?}", outcome.diagnostics);

    // And it obeys the same level machinery as everything else.
    let outcome = Linter::new().with(&lint).deny(NO_INPUT).check("Halt\n");
    assert!(outcome.has_errors());
    let outcome = Linter::new().with(&lint).allow(NO_INPUT).check("Halt\n");
    assert!(!outcome.has(NO_INPUT));
}

#[test]
fn an_empty_linter_runs_only_what_is_registered() {
    let lint = mrs_lint::lints::style::MissingHalt;
    let outcome = Linter::empty().with(&lint).check(MESSY);
    assert_eq!(
        outcome
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        vec![MISSING_HALT],
        "only the registered lint ran"
    );
}

#[test]
fn the_registry_is_listable_and_every_lint_documents_itself() {
    // What a `--list-lints` flag would print.
    let linter = Linter::new();
    assert!(linter.lints().len() >= ALL.len());
    for lint in linter.lints() {
        assert!(
            !lint.description().is_empty(),
            "{} has no description",
            lint.code()
        );
    }
    // The default set includes the assembler's own lints as well as this crate's.
    let codes: Vec<_> = linter.lints().iter().map(|l| l.code()).collect();
    assert!(codes.contains(&FALLS_INTO_DATA), "this crate's lints");
    assert!(codes.contains(&Code::UNUSED_LABEL), "the assembler's lints");
}

#[test]
fn every_shipped_lint_has_a_unique_code_in_this_crates_namespace() {
    let mut codes: Vec<String> = ALL.iter().map(|l| l.code().to_string()).collect();
    codes.sort();
    let total = codes.len();
    codes.dedup();
    assert_eq!(codes.len(), total, "duplicate lint code");
    assert!(ALL.iter().all(|l| l.code().namespace() == mrs_lint::LINT));
}

#[test]
fn assembler_errors_are_never_re_levelled_by_lint_configuration() {
    // A broken file fails no matter what the lint levels say.
    let outcome = Linter::new()
        .default_level(Level::Allow)
        .check("Load Missing\n");
    assert!(outcome.has_errors());
    assert!(!outcome.assembled());
    assert_eq!(
        outcome.errors().map(|d| d.code).collect::<Vec<_>>(),
        vec![Code::UNKNOWN_LABEL]
    );
}
