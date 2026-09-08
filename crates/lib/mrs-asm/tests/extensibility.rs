//! The extension points: custom codes, custom lints, custom diagnostic destinations.
//!
//! These stand in for the crates that will consume this one — a linter and a language
//! server — so that a change which closes one of the extension points fails here.

use mrs_asm::ast::MnemonicKind;
use mrs_asm::diagnostic::{Code, Diagnostic, Filtered, FromFn, Severity, Sink};
use mrs_asm::lint::{Lint, LintContext, UnusedLabel};
use mrs_asm::{Options, assemble, assemble_into, assemble_with};

/// A namespace owned by a hypothetical downstream linter.
const HOUSE: &str = "housestyle";
const NON_CANONICAL: Code = Code::new(HOUSE, "non-canonical-mnemonic");
const BARE_HALT: Code = Code::new(HOUSE, "missing-halt");

/// Flags mnemonics not written in their canonical spelling.
struct Canonical;

impl Lint for Canonical {
    fn code(&self) -> Code {
        NON_CANONICAL
    }

    fn description(&self) -> &'static str {
        "a mnemonic is not written in its canonical spelling"
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        for item in &cx.assembly.program.items {
            let Some(opcode) = item.mnemonic.opcode() else {
                continue;
            };
            if item.mnemonic.text != opcode.mnemonic() {
                out.push(
                    Diagnostic::warning(
                        self.code(),
                        item.mnemonic.span,
                        item.line,
                        format!(
                            "Write `{}` rather than `{}`.",
                            opcode.mnemonic(),
                            item.mnemonic.text
                        ),
                    )
                    .with_help("Canonical spellings are easier to grep for."),
                );
            }
        }
    }
}

/// Flags a program with no `Halt` anywhere.
struct RequiresHalt;

impl Lint for RequiresHalt {
    fn code(&self) -> Code {
        BARE_HALT
    }

    fn run(&self, cx: &LintContext<'_>, out: &mut dyn Sink) {
        let has_halt = cx
            .assembly
            .program
            .items
            .iter()
            .any(|item| item.mnemonic.opcode() == Some(mrs_core::Opcode::Halt));
        if !has_halt && !cx.assembly.program.items.is_empty() {
            out.push(Diagnostic::advice(
                self.code(),
                cx.assembly.program.items[0].span,
                0,
                "This program never halts.",
            ));
        }
    }
}

#[test]
fn a_downstream_crate_can_mint_its_own_codes() {
    assert_eq!(NON_CANONICAL.namespace(), HOUSE);
    assert_eq!(
        NON_CANONICAL.to_string(),
        "housestyle::non-canonical-mnemonic"
    );
    assert!(!NON_CANONICAL.is_builtin());
    // A foreign code with the same bare name as a built-in one is still distinct.
    assert_ne!(Code::new(HOUSE, "unused-label"), Code::UNUSED_LABEL);
}

#[test]
fn a_custom_lint_runs_alongside_the_built_in_ones() {
    let source = "Spare, DEC 1\n        HALT\n";
    let assembly = assemble_with(source, Options::with_lints(&[&UnusedLabel, &Canonical]));

    assert!(assembly.succeeded(), "lints never block assembly");
    let codes: Vec<_> = assembly.warnings().map(|d| d.code).collect();
    assert!(codes.contains(&Code::UNUSED_LABEL), "built-in lint ran");
    assert!(codes.contains(&NON_CANONICAL), "custom lint ran");
}

#[test]
fn a_custom_lint_can_run_on_its_own() {
    let assembly = assemble_with("Load 001", Options::with_lints(&[&Canonical]));
    // The built-in unused-label lint was not registered, so it did not fire.
    assert_eq!(assembly.warnings().count(), 0);
    assert_eq!(assemble("Load 001").warnings().count(), 0);
}

#[test]
fn lints_can_report_at_any_severity_without_blocking_assembly() {
    let assembly = assemble_with("Load 001", Options::with_lints(&[&RequiresHalt]));
    assert!(assembly.succeeded());
    let advice: Vec<_> = assembly
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Advice)
        .collect();
    assert_eq!(advice.len(), 1);
    assert_eq!(advice[0].code, BARE_HALT);
}

#[test]
fn the_default_options_run_no_lints_at_all() {
    // Compatibility checks must see exactly what MARIE.js sees.
    let source = "Spare, DEC 1\nHALT\nEND\ntrailing\n";
    assert_eq!(assemble(source).diagnostics.len(), 0);
    assert_eq!(assemble_with(source, Options::bare()).diagnostics.len(), 0);
    assert!(
        !assemble_with(source, Options::linting())
            .diagnostics
            .is_empty()
    );
}

#[test]
fn diagnostics_can_be_streamed_to_a_closure_instead_of_collected() {
    // What a language server does: convert each diagnostic as it arrives.
    let mut seen: Vec<String> = Vec::new();
    let assembly = {
        let mut sink = FromFn(|d: Diagnostic| seen.push(format!("{}@{}", d.code, d.line)));
        assemble_into("Load Missing\nNope 1\n", &mut sink)
    };
    assert_eq!(
        seen,
        vec!["asm::unknown-label@0", "asm::unknown-mnemonic@1"]
    );
    // The streaming entry point still returns the full assembly for tooling.
    assert_eq!(assembly.program.items.len(), 2);
    assert!(
        assembly.diagnostics.is_empty(),
        "diagnostics went to the sink"
    );
}

#[test]
fn a_filtered_sink_applies_a_user_allow_list() {
    let mut kept: Vec<Diagnostic> = Vec::new();
    {
        // Silence one code, the way a config file would.
        let mut sink = Filtered::new(&mut kept, |d: &Diagnostic| d.code != Code::UNKNOWN_MNEMONIC);
        assemble_into("Load Missing\nNope 1\n", &mut sink);
    }
    assert_eq!(
        kept.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![Code::UNKNOWN_LABEL]
    );
}

#[test]
fn a_sink_can_cap_how_much_it_keeps() {
    // A pathological file should not be able to make an editor allocate without bound.
    let mut kept: Vec<Diagnostic> = Vec::new();
    {
        let mut count = 0;
        let mut sink = Filtered::new(&mut kept, |_: &Diagnostic| {
            count += 1;
            count <= 3
        });
        assemble_into(&"Load Missing\n".repeat(100), &mut sink);
    }
    assert_eq!(kept.len(), 3);
}

#[test]
fn the_lint_registry_is_introspectable() {
    // A tool listing available lints for a config file needs the code and description.
    for lint in mrs_asm::lint::STANDARD {
        assert!(lint.code().is_builtin());
        assert!(
            !lint.description().is_empty(),
            "{} has no description",
            lint.code()
        );
    }
    assert_eq!(UnusedLabel.code(), Code::UNUSED_LABEL);
}

#[test]
fn the_ast_is_reusable_without_touching_the_assembler() {
    // A formatter walks the tree and never calls the second pass.
    let assembly = assemble("Foo, Load Bar / c\nBar, DEC 1\n");
    let shapes: Vec<_> = assembly
        .program
        .items
        .iter()
        .map(|item| {
            (
                item.label.as_ref().map(|l| l.name.as_str()),
                item.mnemonic.text.as_str(),
                item.operand.as_ref().map(|o| o.text.as_str()),
                matches!(item.mnemonic.kind, MnemonicKind::Opcode(_)),
            )
        })
        .collect();
    assert_eq!(
        shapes,
        vec![
            (Some("Foo"), "Load", Some("Bar"), true),
            (Some("Bar"), "DEC", Some("1"), false),
        ]
    );
}
