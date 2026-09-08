//! `marie lint` — check a program for mistakes that assemble cleanly.

use std::path::PathBuf;

use clap::Args as ClapArgs;
use mrs_asm::diagnostic::Code;
use mrs_lint::{Level, Linter};

use crate::{name, read};

/// Arguments to `marie lint`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The source file to check. Not needed with `--list`.
    #[arg(required_unless_present = "list")]
    pub file: Option<PathBuf>,

    /// Silence a lint, by code. May be repeated.
    #[arg(short = 'A', long = "allow", value_name = "CODE")]
    pub allow: Vec<String>,

    /// Treat a lint as an error, by code. May be repeated.
    #[arg(short = 'D', long = "deny", value_name = "CODE")]
    pub deny: Vec<String>,

    /// Treat every lint as an error.
    #[arg(long, conflicts_with = "deny")]
    pub deny_all: bool,

    /// List the available lints and exit.
    #[arg(long)]
    pub list: bool,
}

/// Lints the file and reports the findings.
pub fn run(args: Args) -> miette::Result<()> {
    if args.list {
        list_lints();
        return Ok(());
    }

    let file = args
        .file
        .clone()
        .expect("clap requires a file unless --list was given");
    let source = read(&file)?;
    let linter = configure(&args)?;
    let outcome = linter.check(&source);

    if outcome.diagnostics.is_empty() {
        println!("{}: no findings", name(&file));
        return Ok(());
    }

    let report = outcome.report(name(&file), &source);
    if outcome.has_errors() {
        return Err(report.into());
    }
    println!("{:?}", miette::Report::new(report));
    Ok(())
}

/// Applies the `--allow` and `--deny` flags to the standard lint set.
fn configure(args: &Args) -> miette::Result<Linter<'static>> {
    let mut linter = Linter::new();
    if args.deny_all {
        linter = linter.default_level(Level::Deny);
    }
    for name in &args.allow {
        linter = linter.set(resolve(name)?, Level::Allow);
    }
    for name in &args.deny {
        linter = linter.set(resolve(name)?, Level::Deny);
    }
    Ok(linter)
}

/// Resolves a lint name written on the command line to its code.
///
/// Both the bare name and the fully qualified `namespace::name` are accepted, so
/// `--deny falls-into-data` and `--deny lint::falls-into-data` mean the same thing.
fn resolve(written: &str) -> miette::Result<Code> {
    let known = known_codes();
    for code in &known {
        if code.name() == written || code.to_string() == written {
            return Ok(*code);
        }
    }
    let mut names: Vec<_> = known.iter().map(|code| code.name()).collect();
    names.sort_unstable();
    Err(miette::miette!(
        help = format!("known lints: {}", names.join(", ")),
        "unknown lint '{written}'"
    ))
}

/// Every code that can be configured: the lints, not the assembler's errors.
fn known_codes() -> Vec<Code> {
    Linter::new()
        .lints()
        .iter()
        .map(|lint| lint.code())
        .collect()
}

/// Prints the lint table for `--list`.
fn list_lints() {
    let linter = Linter::new();
    let mut lints: Vec<_> = linter
        .lints()
        .iter()
        .map(|lint| (lint.code(), lint.description()))
        .collect();
    lints.sort_by_key(|(code, _)| code.to_string());

    let width = lints
        .iter()
        .map(|(code, _)| code.to_string().len())
        .max()
        .unwrap_or(0);
    for (code, description) in lints {
        println!("{:width$}  {description}", code.to_string());
    }
}
