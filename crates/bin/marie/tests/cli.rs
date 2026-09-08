//! The `marie` binary, run as a user runs it.
//!
//! These invoke the real executable rather than calling into the subcommand modules, so
//! argument parsing, exit codes and what actually reaches stdout are all covered.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// Writes a source file into a unique temporary directory.
struct Fixture {
    directory: PathBuf,
}

impl Fixture {
    fn new(test: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("marie-cli-{}-{test}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temp directory");
        Self { directory }
    }

    /// Writes `source` to `name` and returns its path.
    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.directory.join(name);
        std::fs::write(&path, source).expect("write fixture");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Runs `marie` with the given arguments.
fn marie(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_marie"))
        .args(arguments)
        .output()
        .expect("marie should run")
}

/// Runs `marie`, feeding `input` to its stdin.
fn marie_with_stdin(arguments: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_marie"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("marie should start");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("marie should finish")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Flattens a rendered report so a substring assertion is not defeated by the
/// renderer's line wrapping and box drawing.
fn flatten(text: &str) -> String {
    text.replace(['\u{2502}', '\u{2500}'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A program that assembles, runs, and produces no findings.
const GOOD: &str = "\
        Load  First
        Add   Second
        Output
        Halt
First,  DEC 21
Second, DEC 21
";

// ---------------------------------------------------------------------------
// asm
// ---------------------------------------------------------------------------

#[test]
fn asm_reports_success_and_the_word_count() {
    let fixture = Fixture::new("asm-ok");
    let path = fixture.write("good.mas", GOOD);
    let output = marie(&["asm", path.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("assembled 6 words"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn asm_listing_shows_address_word_and_source() {
    let fixture = Fixture::new("asm-listing");
    let path = fixture.write("good.mas", GOOD);
    let output = marie(&["asm", path.to_str().unwrap(), "--listing"]);
    let text = stdout(&output);
    assert!(text.contains("000  1004"), "{text}");
    assert!(text.contains("Load  First"), "{text}");
    assert!(text.contains("004  0015  First,  DEC 21"), "{text}");
}

#[test]
fn asm_hex_prints_one_word_per_line() {
    let fixture = Fixture::new("asm-hex");
    let path = fixture.write("good.mas", GOOD);
    let output = marie(&["asm", path.to_str().unwrap(), "--hex"]);
    let lines: Vec<_> = stdout(&output).lines().map(str::to_owned).collect();
    assert_eq!(lines, vec!["1004", "3005", "6000", "7000", "0015", "0015"]);
}

#[test]
fn asm_fails_on_a_broken_program_and_names_the_problem() {
    let fixture = Fixture::new("asm-bad");
    let path = fixture.write("bad.mas", "        Load  Missing\n");
    let output = marie(&["asm", path.to_str().unwrap()]);
    assert!(!output.status.success(), "should exit non-zero");
    assert!(
        stderr(&output).contains("Unknown label 'Missing'"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn asm_reports_a_missing_file_rather_than_panicking() {
    let output = marie(&["asm", "/nonexistent/nope.mas"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("could not read"),
        "{}",
        stderr(&output)
    );
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

#[test]
fn run_executes_the_program_and_prints_its_output() {
    let fixture = Fixture::new("run-ok");
    let path = fixture.write("good.mas", GOOD);
    let output = marie(&["run", path.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("42"), "{}", stdout(&output));
}

#[test]
fn run_reads_from_stdin() {
    let fixture = Fixture::new("run-input");
    let path = fixture.write("echo.mas", "        Input\n        Output\n        Halt\n");
    let output = marie_with_stdin(&["run", path.to_str().unwrap()], "17\n");
    assert!(stdout(&output).contains("17"), "{}", stdout(&output));
}

#[test]
fn run_stops_a_runaway_program_instead_of_hanging() {
    let fixture = Fixture::new("run-spin");
    let path = fixture.write("spin.mas", "Spin,   Jump Spin\n");
    let output = marie(&["run", path.to_str().unwrap(), "--max-steps", "1000"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("step limit"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn run_refuses_a_program_that_does_not_assemble() {
    let fixture = Fixture::new("run-bad");
    let path = fixture.write("bad.mas", "        Load  Missing\n");
    let output = marie(&["run", path.to_str().unwrap()]);
    assert!(!output.status.success());
}

// ---------------------------------------------------------------------------
// lint
// ---------------------------------------------------------------------------

/// A program that assembles but falls into its data and shouts its mnemonic.
const MESSY: &str = "        LOAD  X\n        Add   X\nX,      DEC 21\n";

#[test]
fn lint_says_nothing_about_a_clean_program() {
    let fixture = Fixture::new("lint-clean");
    let path = fixture.write("good.mas", GOOD);
    let output = marie(&["lint", path.to_str().unwrap()]);
    assert!(output.status.success());
    assert!(
        stdout(&output).contains("no findings"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn lint_reports_findings_without_failing() {
    let fixture = Fixture::new("lint-messy");
    let path = fixture.write("messy.mas", MESSY);
    let output = marie(&["lint", path.to_str().unwrap()]);
    assert!(output.status.success(), "warnings alone must not fail");
    assert!(
        stdout(&output).contains("falls-into-data"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn lint_deny_turns_a_finding_into_a_failure() {
    let fixture = Fixture::new("lint-deny");
    let path = fixture.write("messy.mas", MESSY);
    let output = marie(&["lint", path.to_str().unwrap(), "--deny", "falls-into-data"]);
    assert!(
        !output.status.success(),
        "a denied lint should fail the run"
    );
}

#[test]
fn lint_allow_silences_a_finding() {
    let fixture = Fixture::new("lint-allow");
    let path = fixture.write("messy.mas", MESSY);
    let output = marie(&[
        "lint",
        path.to_str().unwrap(),
        "--allow",
        "falls-into-data",
        "--allow",
        "non-canonical-mnemonic",
        "--allow",
        "falls-off-end",
        "--allow",
        "missing-halt",
    ]);
    assert!(output.status.success());
    assert!(
        stdout(&output).contains("no findings"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn lint_accepts_a_fully_qualified_code() {
    let fixture = Fixture::new("lint-qualified");
    let path = fixture.write("messy.mas", MESSY);
    let output = marie(&[
        "lint",
        path.to_str().unwrap(),
        "--deny",
        "lint::falls-into-data",
    ]);
    assert!(!output.status.success());
}

#[test]
fn lint_rejects_an_unknown_code_and_suggests_the_real_ones() {
    let fixture = Fixture::new("lint-unknown");
    let path = fixture.write("good.mas", GOOD);
    let output = marie(&["lint", path.to_str().unwrap(), "--deny", "nonsense"]);
    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(text.contains("unknown lint 'nonsense'"), "{text}");
    assert!(
        text.contains("falls-into-data"),
        "lists the real ones:\n{text}"
    );
}

#[test]
fn lint_list_works_without_a_file() {
    let output = marie(&["lint", "--list"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("lint::masked-skipcond"), "{text}");
    assert!(
        text.contains("asm::unused-label"),
        "includes the assembler's own"
    );
}

// ---------------------------------------------------------------------------
// debug
// ---------------------------------------------------------------------------

#[test]
fn debug_steps_forwards_and_backwards() {
    let fixture = Fixture::new("debug-step");
    let path = fixture.write("good.mas", GOOD);
    let output = marie_with_stdin(
        &["debug", path.to_str().unwrap()],
        "step\nstep\nregs\nback\nregs\nquit\n",
    );
    let text = stdout(&output);
    // After two instructions the accumulator holds 21 + 21.
    assert!(text.contains("AC=002A"), "{text}");
    // After stepping back it holds only the first load.
    assert!(text.contains("AC=0015"), "{text}");
}

#[test]
fn debug_single_steps_micro_operations_and_undoes_them() {
    let fixture = Fixture::new("debug-micro");
    let path = fixture.write("good.mas", GOOD);
    let output = marie_with_stdin(
        &["debug", path.to_str().unwrap()],
        "stepi\nstepi\nbacki\nquit\n",
    );
    let text = stdout(&output);
    assert!(
        text.contains("MAR <- PC"),
        "shows register transfers:\n{text}"
    );
    assert!(text.contains("undid"), "and reverses them:\n{text}");
}

#[test]
fn debug_honours_breakpoints() {
    let fixture = Fixture::new("debug-break");
    let path = fixture.write("good.mas", GOOD);
    let output = marie_with_stdin(
        &["debug", path.to_str().unwrap()],
        "break 003\ncontinue\nregs\nquit\n",
    );
    let text = stdout(&output);
    assert!(text.contains("breakpoint at 003"), "{text}");
    assert!(
        text.contains("PC=003"),
        "stopped before executing it:\n{text}"
    );
}

#[test]
fn debug_can_set_a_breakpoint_from_the_command_line() {
    let fixture = Fixture::new("debug-break-arg");
    let path = fixture.write("good.mas", GOOD);
    let output = marie_with_stdin(
        &["debug", path.to_str().unwrap(), "--break", "002"],
        "continue\nquit\n",
    );
    assert!(
        stdout(&output).contains("breakpoint at 002"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn debug_dumps_memory() {
    let fixture = Fixture::new("debug-mem");
    let path = fixture.write("good.mas", GOOD);
    let output = marie_with_stdin(&["debug", path.to_str().unwrap()], "mem 004\nquit\n");
    // 21 decimal is 0x0015, stored twice.
    assert!(stdout(&output).contains("0015 0015"), "{}", stdout(&output));
}

#[test]
fn debug_rejects_a_bad_breakpoint_address() {
    let fixture = Fixture::new("debug-bad-break");
    let path = fixture.write("good.mas", GOOD);
    let output = marie_with_stdin(&["debug", path.to_str().unwrap()], "break zzz\nquit\n");
    assert!(
        stdout(&output).contains("not a hexadecimal address"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn debug_survives_an_unknown_command() {
    let fixture = Fixture::new("debug-unknown");
    let path = fixture.write("good.mas", GOOD);
    let output = marie_with_stdin(&["debug", path.to_str().unwrap()], "wat\nstep\nquit\n");
    assert!(output.status.success());
    assert!(
        stdout(&output).contains("unknown command"),
        "{}",
        stdout(&output)
    );
}

// ---------------------------------------------------------------------------
// General
// ---------------------------------------------------------------------------

#[test]
fn help_lists_every_subcommand() {
    let output = marie(&["--help"]);
    let text = stdout(&output);
    for subcommand in ["asm", "run", "lint", "debug"] {
        assert!(text.contains(subcommand), "missing {subcommand}:\n{text}");
    }
}

#[test]
fn no_arguments_is_an_error_with_usage() {
    let output = marie(&[]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("Usage"), "{}", stderr(&output));
}

// ---------------------------------------------------------------------------
// Binary memory images
// ---------------------------------------------------------------------------

/// A program with an `ORG`, so relocation is exercised too.
const RELOCATED: &str = "\
ORG 100
        Load  X
        Add   X
        Output
        Halt
X,      DEC 21
";

#[test]
fn asm_writes_a_full_memory_image_in_the_marie_js_format() {
    let fixture = Fixture::new("bin-write");
    let source = fixture.write("good.mas", GOOD);
    let binary = fixture.directory.join("good.bin");

    let output = marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let bytes = std::fs::read(&binary).expect("the image");
    // MARIE.js writes the whole address space: 4096 words of two bytes.
    assert_eq!(bytes.len(), 8192);
    // And little-endian, so `Load 004` = 0x1004 is stored low byte first. Reading this
    // big-endian would give 0x0410, a jump to the wrong address with no error.
    assert_eq!(&bytes[0..2], &[0x04, 0x10]);
    assert_eq!(&bytes[2..4], &[0x05, 0x30], "Add 005 = 0x3005");
    // Everything past the program is zero.
    assert!(bytes[12..].iter().all(|byte| *byte == 0));
}

#[test]
fn asm_bare_writes_only_the_program() {
    let fixture = Fixture::new("bin-bare");
    let source = fixture.write("good.mas", GOOD);
    let binary = fixture.directory.join("good.bin");

    let output = marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
        "--bare",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    // Six words rather than the whole address space.
    assert_eq!(std::fs::read(&binary).unwrap().len(), 12);
}

#[test]
fn a_binary_image_round_trips_through_run() {
    let fixture = Fixture::new("bin-round-trip");
    let source = fixture.write("good.mas", GOOD);
    let binary = fixture.directory.join("good.bin");

    marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
    ]);
    let output = marie(&["run", binary.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("42"), "{}", stdout(&output));
}

#[test]
fn a_full_image_takes_origin_as_its_entry_point() {
    // The program sits at 0x100 inside a whole-memory dump, which can only load at
    // zero, so `--origin` has to mean "start here" rather than "put it here".
    let fixture = Fixture::new("bin-entry");
    let source = fixture.write("org.mas", RELOCATED);
    let binary = fixture.directory.join("org.bin");

    marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
    ]);
    let output = marie(&["run", binary.to_str().unwrap(), "--origin", "100"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("42"), "{}", stdout(&output));
}

#[test]
fn a_bare_image_is_loaded_at_the_origin() {
    let fixture = Fixture::new("bin-bare-origin");
    let source = fixture.write("org.mas", RELOCATED);
    let binary = fixture.directory.join("org.bin");

    marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
        "--bare",
    ]);
    let output = marie(&["run", binary.to_str().unwrap(), "--origin", "100"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("42"), "{}", stdout(&output));
}

#[test]
fn the_format_can_be_forced_for_a_file_with_the_wrong_extension() {
    let fixture = Fixture::new("bin-format");
    let source = fixture.write("good.mas", GOOD);
    let binary = fixture.directory.join("good.dat");

    marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
    ]);
    // Without the flag the extension says "assembly", and assembling binary fails.
    let guessed = marie(&["run", binary.to_str().unwrap()]);
    assert!(!guessed.status.success());
    // With it, the file loads.
    let forced = marie(&["run", binary.to_str().unwrap(), "--format", "bin"]);
    assert!(forced.status.success(), "{}", stderr(&forced));
    assert!(stdout(&forced).contains("42"));
}

#[test]
fn an_odd_length_image_is_rejected_with_an_explanation() {
    let fixture = Fixture::new("bin-odd");
    let path = fixture.directory.join("odd.bin");
    std::fs::write(&path, [0x01, 0x02, 0x03]).expect("write");
    let output = marie(&["run", path.to_str().unwrap()]);
    assert!(!output.status.success());
    let text = flatten(&stderr(&output));
    assert!(text.contains("whole number of 16-bit words"), "{text}");
    assert!(text.contains("little-endian"), "names the format: {text}");
}

#[test]
fn an_oversized_image_is_rejected() {
    let fixture = Fixture::new("bin-big");
    let path = fixture.directory.join("big.bin");
    std::fs::write(&path, vec![0u8; 8194]).expect("write");
    let output = marie(&["run", path.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("address space"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_bare_image_that_does_not_fit_at_its_origin_is_rejected() {
    let fixture = Fixture::new("bin-overflow");
    let path = fixture.directory.join("frag.bin");
    // Four words placed two words from the end of memory.
    std::fs::write(&path, vec![0u8; 8]).expect("write");
    let output = marie(&["run", path.to_str().unwrap(), "--origin", "FFE"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("does not fit"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn debug_accepts_a_binary_image_and_falls_back_to_disassembly() {
    let fixture = Fixture::new("bin-debug");
    let source = fixture.write("good.mas", GOOD);
    let binary = fixture.directory.join("good.bin");
    marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
    ]);

    let output = marie_with_stdin(
        &["debug", binary.to_str().unwrap()],
        "step\nstep\nregs\nback\nregs\nquit\n",
    );
    let text = stdout(&output);
    assert!(text.contains("binary image"), "says so up front:\n{text}");
    // No source lines, so it shows the decoded instruction instead.
    assert!(text.contains("Load 004"), "{text}");
    // Reverse execution still works without any source.
    assert!(
        text.contains("AC=002A") && text.contains("AC=0015"),
        "{text}"
    );
}

// ---------------------------------------------------------------------------
// disasm
// ---------------------------------------------------------------------------

#[test]
fn disasm_decodes_an_image_and_trims_the_trailing_zeros() {
    let fixture = Fixture::new("disasm");
    let source = fixture.write("good.mas", GOOD);
    let binary = fixture.directory.join("good.bin");
    marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
    ]);

    let output = marie(&["disasm", binary.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("000  1004  Load 004"), "{text}");
    assert!(text.contains("003  7000  Halt"), "{text}");
    // A data word is shown as both an instruction and a number, so a reader can tell.
    assert!(text.contains("(21)"), "shows decimal values:\n{text}");
    assert!(text.contains("zero words omitted"), "{text}");
}

#[test]
fn disasm_all_prints_the_whole_address_space() {
    let fixture = Fixture::new("disasm-all");
    let source = fixture.write("good.mas", GOOD);
    let binary = fixture.directory.join("good.bin");
    marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
    ]);

    let output = marie(&["disasm", binary.to_str().unwrap(), "--all"]);
    assert_eq!(stdout(&output).lines().count(), 4096);
}

#[test]
fn disasm_honours_the_origin() {
    let fixture = Fixture::new("disasm-origin");
    let source = fixture.write("good.mas", GOOD);
    let binary = fixture.directory.join("good.bin");
    marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
        "--bare",
    ]);
    let output = marie(&["disasm", binary.to_str().unwrap(), "--origin", "200"]);
    assert!(stdout(&output).contains("200  1004"), "{}", stdout(&output));
}

#[test]
fn disasm_reports_an_empty_image_rather_than_printing_nothing() {
    let fixture = Fixture::new("disasm-empty");
    let path = fixture.directory.join("empty.bin");
    std::fs::write(&path, vec![0u8; 8192]).expect("write");
    let output = marie(&["disasm", path.to_str().unwrap()]);
    assert!(output.status.success());
    assert!(
        stdout(&output).contains("empty image"),
        "{}",
        stdout(&output)
    );
}

// ---------------------------------------------------------------------------
// Step budget and the memory-mapped display
// ---------------------------------------------------------------------------

/// Fills the first pixel with pure red, the second with pure green.
const DRAWS: &str = "\
        Load  Red
        Store 0F00
        Load  Green
        Store 0F01
        Halt
Red,    HEX 7C00
Green,  HEX 03E0
";

#[test]
fn run_is_unlimited_by_default() {
    // A program needing more than the old ten-million-instruction cap must now finish
    // without being told to.
    let fixture = Fixture::new("unlimited");
    // Count down from 4000, which is a few tens of thousands of instructions, and would
    // have been fine before; the point is that no budget is imposed at all.
    let path = fixture.write(
        "count.mas",
        "\
        Load  N
Loop,   Subt  One
        Store N
        Skipcond 800
        Jump  Done
        Load  N
        Jump  Loop
Done,   Halt
N,      DEC 4000
One,    DEC 1
",
    );
    let output = marie(&["run", path.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));

    // The flag still works when it is asked for.
    let spin = fixture.write("spin.mas", "Spin,   Jump Spin\n");
    let limited = marie(&["run", spin.to_str().unwrap(), "--max-steps", "1000"]);
    assert!(!limited.status.success());
    assert!(
        stderr(&limited).contains("step limit"),
        "{}",
        stderr(&limited)
    );
}

#[test]
fn run_display_draws_the_pixels_a_program_writes() {
    let fixture = Fixture::new("display");
    let path = fixture.write("draw.mas", DRAWS);
    let output = marie(&["run", path.to_str().unwrap(), "--display"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let text = stdout(&output);
    // Truecolor escapes for pure red and pure green. The five-bit channels widen by
    // scaling, so full brightness is 255 — `31 << 3` would print 248 here.
    assert!(text.contains("\u{1b}[38;2;255;0;0m"), "no red pixel");
    assert!(text.contains("\u{1b}[38;2;0;255;0m"), "no green pixel");
    // Sixteen rows per frame.
    assert!(text.contains("\u{1b}[0m"), "rows are reset");
}

#[test]
fn a_program_that_draws_nothing_renders_a_black_display() {
    let fixture = Fixture::new("display-blank");
    let path = fixture.write("good.mas", GOOD);
    let output = marie(&["run", path.to_str().unwrap(), "--display"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("\u{1b}[38;2;0;0;0m"), "black pixels");
    assert!(!text.contains("255;0;0"), "nothing was drawn");
}

#[test]
fn the_display_works_from_a_binary_image_too() {
    let fixture = Fixture::new("display-bin");
    let source = fixture.write("draw.mas", DRAWS);
    let binary = fixture.directory.join("draw.bin");
    marie(&[
        "asm",
        source.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
    ]);

    let output = marie(&["run", binary.to_str().unwrap(), "--display"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("\u{1b}[38;2;255;0;0m"));
}

#[test]
fn refresh_requires_display() {
    let fixture = Fixture::new("display-refresh");
    let path = fixture.write("draw.mas", DRAWS);
    let output = marie(&["run", path.to_str().unwrap(), "--refresh", "100"]);
    assert!(!output.status.success(), "--refresh alone is meaningless");
}

#[test]
fn debug_can_draw_the_display_on_demand() {
    let fixture = Fixture::new("debug-display");
    let path = fixture.write("draw.mas", DRAWS);
    let output = marie_with_stdin(
        &["debug", path.to_str().unwrap()],
        "step\nstep\ndisplay\nquit\n",
    );
    let text = stdout(&output);
    assert!(
        text.contains("\u{1b}[38;2;255;0;0m"),
        "the red pixel: {text:?}"
    );
}

#[test]
fn debug_display_flag_follows_the_picture_as_it_steps() {
    let fixture = Fixture::new("debug-follow");
    let path = fixture.write("draw.mas", DRAWS);
    let output = marie_with_stdin(
        &["debug", path.to_str().unwrap(), "--display"],
        "step\nstep\nquit\n",
    );
    assert!(stdout(&output).contains("\u{1b}[38;2;255;0;0m"));
}

#[test]
fn stepping_back_in_the_debugger_unpaints_a_pixel() {
    let fixture = Fixture::new("debug-unpaint");
    let path = fixture.write("draw.mas", DRAWS);
    // Draw the red pixel, then rewind past the Store and look again.
    let output = marie_with_stdin(
        &["debug", path.to_str().unwrap()],
        "step\nstep\ndisplay\nback\ndisplay\nquit\n",
    );
    let text = stdout(&output);
    let frames: Vec<&str> = text.split("(marie)").collect();
    let drawn = frames.iter().filter(|f| f.contains("38;2;255;0;0")).count();
    assert_eq!(
        drawn, 1,
        "the pixel is painted once and then undone:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// Speed
// ---------------------------------------------------------------------------

/// Two instructions: `Load X; Halt`, which is thirteen paced micro-operations.
const TINY: &str = "        Load  X\n        Halt\nX,      DEC 7\n";

#[test]
fn the_speed_levels_can_be_listed_without_a_file() {
    let output = marie(&["run", "--speeds"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let lines: Vec<_> = stdout(&output).lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 10, "ten slider positions");
    assert!(lines[0].contains("1 step every 1000 ms"), "{}", lines[0]);
    assert!(lines[9].contains("unlimited"), "{}", lines[9]);
}

#[test]
fn an_out_of_range_speed_is_rejected() {
    let fixture = Fixture::new("speed-range");
    let path = fixture.write("tiny.mas", TINY);
    for level in ["10", "-1", "fast"] {
        let output = marie(&["run", path.to_str().unwrap(), "--speed", level]);
        assert!(!output.status.success(), "speed {level} should be rejected");
    }
}

#[test]
fn the_top_speed_runs_without_pacing() {
    let fixture = Fixture::new("speed-fast");
    let path = fixture.write("tiny.mas", TINY);
    let started = std::time::Instant::now();
    let output = marie(&["run", path.to_str().unwrap(), "--speed", "9"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "the unlimited level should not pace"
    );
}

#[test]
fn a_slow_speed_actually_slows_the_program_down() {
    // Level 3 delays 10 ms per micro-operation, and this program is thirteen of them,
    // so it cannot finish instantly. The bound is loose because it is a wall-clock
    // assertion, but the difference from unpaced is two orders of magnitude.
    let fixture = Fixture::new("speed-slow");
    let path = fixture.write("tiny.mas", TINY);

    let started = std::time::Instant::now();
    let output = marie(&["run", path.to_str().unwrap(), "--speed", "3"]);
    let elapsed = started.elapsed();

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        elapsed >= std::time::Duration::from_millis(60),
        "expected pacing, finished in {elapsed:?}"
    );
}

#[test]
fn a_paced_run_still_reaches_the_end_and_reports_registers() {
    let fixture = Fixture::new("speed-registers");
    let path = fixture.write("tiny.mas", TINY);
    let output = marie(&["run", path.to_str().unwrap(), "--speed", "5", "--registers"]);
    assert!(output.status.success(), "{}", stderr(&output));
    // `Load X` put 7 in the accumulator, so the machine really executed.
    assert!(stdout(&output).contains("AC=0007"), "{}", stdout(&output));
}

#[test]
fn pacing_and_the_display_work_together() {
    let fixture = Fixture::new("speed-display");
    let path = fixture.write("draw.mas", DRAWS);
    let output = marie(&["run", path.to_str().unwrap(), "--speed", "6", "--display"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("\u{1b}[38;2;255;0;0m"),
        "the red pixel"
    );
}

#[test]
fn a_paced_run_still_honours_the_step_limit() {
    let fixture = Fixture::new("speed-limit");
    let path = fixture.write("spin.mas", "Spin,   Jump Spin\n");
    let output = marie(&[
        "run",
        path.to_str().unwrap(),
        "--speed",
        "8",
        "--max-steps",
        "50",
    ]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("step limit"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_paced_run_reports_a_fault() {
    let fixture = Fixture::new("speed-fault");
    // Opcode 0xF is unassigned, so executing it faults.
    let path = fixture.write("bad.mas", "        HEX F000\n");
    let output = marie(&["run", path.to_str().unwrap(), "--speed", "8"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("invalid opcode"),
        "{}",
        stderr(&output)
    );
}

// ---------------------------------------------------------------------------
// Ctrl-C
// ---------------------------------------------------------------------------

/// Sends SIGINT to a running child.
#[cfg(unix)]
fn interrupt(child: &std::process::Child) {
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill should run");
    assert!(status.success(), "could not signal the child");
}

/// Waits for a child, failing rather than hanging if it ignores the signal.
#[cfg(unix)]
fn wait_briefly(child: &mut std::process::Child, what: &str) -> std::process::ExitStatus {
    for _ in 0..100 {
        if let Some(status) = child.try_wait().expect("wait") {
            return status;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    panic!("{what} did not stop within five seconds");
}

/// Spawns `marie` with the given arguments and a piped stderr.
#[cfg(unix)]
fn spawn(arguments: &[&str]) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_marie"))
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("marie should start")
}

#[cfg(unix)]
#[test]
fn ctrl_c_stops_a_slow_paced_run_promptly() {
    // The slowest level pauses a full second between micro-operations, so a naive
    // sleep would swallow the key press for up to that long.
    let fixture = Fixture::new("interrupt-slow");
    let path = fixture.write("spin.mas", "Spin,   Jump Spin\n");
    let mut child = spawn(&["run", path.to_str().unwrap(), "--speed", "0"]);
    std::thread::sleep(std::time::Duration::from_millis(500));

    let started = std::time::Instant::now();
    interrupt(&child);
    let status = wait_briefly(&mut child, "a paced run");
    let elapsed = started.elapsed();

    assert_eq!(
        status.code(),
        Some(130),
        "the conventional status for SIGINT"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(750),
        "took {elapsed:?}, longer than one delay"
    );
}

#[cfg(unix)]
#[test]
fn ctrl_c_stops_an_unpaced_run_and_says_how_far_it_got() {
    let fixture = Fixture::new("interrupt-fast");
    let path = fixture.write("spin.mas", "Spin,   Jump Spin\n");
    let mut child = spawn(&["run", path.to_str().unwrap()]);
    std::thread::sleep(std::time::Duration::from_millis(400));
    interrupt(&child);

    let status = wait_briefly(&mut child, "an unpaced run");
    assert_eq!(status.code(), Some(130));

    let mut message = String::new();
    use std::io::Read;
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut message)
        .expect("read stderr");
    let message = flatten(&message);
    assert!(message.contains("interrupted at"), "{message}");
    // An unpaced run gets through millions of instructions in that time, and the
    // report should say so rather than claiming zero.
    assert!(
        message.contains("instructions"),
        "should report progress: {message}"
    );
}

#[cfg(unix)]
#[test]
fn ctrl_c_stops_a_run_that_is_drawing_the_display() {
    let fixture = Fixture::new("interrupt-display");
    let path = fixture.write("spin.mas", "Spin,   Jump Spin\n");
    let mut child = spawn(&["run", path.to_str().unwrap(), "--display", "--speed", "6"]);
    std::thread::sleep(std::time::Duration::from_millis(400));
    interrupt(&child);
    assert_eq!(
        wait_briefly(&mut child, "an animated run").code(),
        Some(130)
    );
}

#[cfg(unix)]
#[test]
fn ctrl_c_interrupts_continue_without_ending_the_debug_session() {
    // In a debugger, Ctrl-C means "come back to the prompt", not "quit".
    use std::io::{BufRead, BufReader, Write};

    let fixture = Fixture::new("interrupt-debug");
    let path = fixture.write("spin.mas", "Spin,   Jump Spin\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_marie"))
        .args(["debug", path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("marie should start");

    let mut stdin = child.stdin.take().expect("stdin");
    writeln!(stdin, "continue").expect("write");
    stdin.flush().expect("flush");
    std::thread::sleep(std::time::Duration::from_millis(600));

    interrupt(&child);
    std::thread::sleep(std::time::Duration::from_millis(400));

    // The session must still be answering commands.
    writeln!(stdin, "regs").expect("the session should still be alive");
    writeln!(stdin, "quit").expect("write");
    stdin.flush().expect("flush");
    drop(stdin);

    let status = wait_briefly(&mut child, "the debugger");
    assert_eq!(status.code(), Some(0), "quitting normally, not killed");

    let output: Vec<String> = BufReader::new(child.stdout.take().expect("stdout"))
        .lines()
        .map_while(Result::ok)
        .collect();
    let text = output.join("\n");
    assert!(text.contains("interrupted"), "said so: {text}");
    assert!(text.contains("AC="), "answered `regs` afterwards: {text}");
}
