//! Runs the real `lymra` executable, to check exit codes and where output goes.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SUCCESS: i32 = 0;
const REJECTED: i32 = 1;
const FAILED: i32 = 2;
/// The exit status of a Lymra program stopped by a runtime error.
const RUNTIME_ERROR: i32 = 70;

/// Runs `lymra` from the repository root, so paths in its output are the ones a user would type.
fn lymra(arguments: &[&str]) -> Output {
    lymra_command(arguments)
        .output()
        .expect("the lymra executable runs")
}

fn lymra_command(arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lymra"));
    command
        .args(arguments)
        .current_dir(common::repository_root());
    command
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Writes a scratch source file where Cargo keeps test output, and returns its path.
fn scratch_file(name: &str, contents: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    fs::write(&path, contents).expect("the scratch file can be written");
    path
}

#[test]
fn a_valid_program_passes_the_check() {
    let output = lymra(&["check", "examples/hello.lym"]);
    assert_eq!(output.status.code(), Some(SUCCESS));
    assert_eq!(text(&output.stdout), "examples/hello.lym: no errors\n");
    assert_eq!(text(&output.stderr), "");
}

#[test]
fn parse_prints_the_tree_to_standard_output() {
    let output = lymra(&["parse", "examples/hello.lym"]);
    assert_eq!(output.status.code(), Some(SUCCESS));
    let expected =
        common::read(&common::repository_root().join("compiler/tests/ui/pass/hello.tree"));
    assert_eq!(text(&output.stdout), expected);
}

#[test]
fn a_rejected_program_reports_on_standard_error_and_exits_with_one() {
    let path = scratch_file(
        "rejected.lym",
        b"function main()\n    string name = \"Kero\"\n",
    );
    let output = lymra(&["check", &path.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(REJECTED));
    assert_eq!(text(&output.stdout), "");
    let report = text(&output.stderr);
    assert!(
        report.starts_with(
            "error[L0203]: this declaration needs `variable` or `constant` in front\n"
        ),
        "{report}"
    );
    assert!(report.contains("found 1 error in "), "{report}");
}

#[test]
fn a_file_that_is_not_utf8_is_reported_with_its_line() {
    let path = scratch_file("latin1.lym", b"# first line\n# caf\xE9\n");
    let output = lymra(&["check", &path.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(FAILED));
    let report = text(&output.stderr);
    assert!(report.starts_with("error[L0115]: "), "{report}");
    assert!(
        report.contains("the first invalid byte is on line 2"),
        "{report}"
    );
}

#[test]
fn explain_prints_the_page_from_the_docs() {
    let output = lymra(&["explain", "L0203"]);
    assert_eq!(output.status.code(), Some(SUCCESS));
    let page = common::read(&common::repository_root().join("docs/errors/L0203.md"));
    assert_eq!(text(&output.stdout), page);
}

#[test]
fn a_wrong_command_line_exits_with_two() {
    for arguments in [
        &[][..],
        &["build"],
        &["check"],
        &["check", "notes.txt"],
        &["check", "missing.lym"],
    ] {
        let output = lymra(arguments);
        assert_eq!(output.status.code(), Some(FAILED), "for {arguments:?}");
        assert_eq!(text(&output.stdout), "", "for {arguments:?}");
    }
}

#[test]
fn run_builds_and_runs_a_program() {
    let output = lymra(&["run", "examples/hello.lym"]);
    assert_eq!(text(&output.stderr), "");
    assert_eq!(
        text(&output.stdout),
        "Hello, Cresmar Mat-an\nSample 1\n1000000\n"
    );
    assert_eq!(output.status.code(), Some(SUCCESS));
}

#[test]
fn run_passes_on_a_runtime_error() {
    let path = scratch_file(
        "overflow.lym",
        b"function main()\n    variable unsigned8 level = 255\n    level = level + 1\n    print(\"{level}\")\n",
    );
    let output = lymra(&["run", &path.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(RUNTIME_ERROR));
    let report = text(&output.stderr);
    assert!(report.starts_with("error: integer overflow\n"), "{report}");
    assert!(report.contains("level = level + 1"), "{report}");
}

#[test]
fn build_writes_an_executable_next_to_the_source() {
    let path = scratch_file(
        "built.lym",
        b"function main()\n    print(\"built and run\")\n",
    );
    let executable = path.with_extension(std::env::consts::EXE_EXTENSION);
    let output = lymra(&["build", &path.to_string_lossy()]);
    assert_eq!(
        output.status.code(),
        Some(SUCCESS),
        "{}",
        text(&output.stderr)
    );
    assert!(text(&output.stdout).contains(": built "));

    let program = Command::new(&executable)
        .output()
        .expect("the built program runs");
    assert_eq!(text(&program.stdout), "built and run\n");
    fs::remove_file(&executable).expect("the executable can be removed");
}

#[test]
fn emit_prints_the_generated_c() {
    let output = lymra(&["emit", "examples/hello.lym"]);
    assert_eq!(output.status.code(), Some(SUCCESS));
    let c_source = text(&output.stdout);
    assert!(c_source.starts_with("/* Generated by the Lymra compiler from examples/hello.lym."));
    assert!(c_source.contains("static void function_main(void)"));
    assert!(c_source.contains("int main(void) {"));
}

#[test]
fn a_program_without_main_can_be_checked_but_not_run() {
    let path = scratch_file("library.lym", b"public constant integer64 answer = 42\n");
    let checked = lymra(&["check", &path.to_string_lossy()]);
    assert_eq!(checked.status.code(), Some(SUCCESS));

    let run = lymra(&["run", &path.to_string_lossy()]);
    assert_eq!(run.status.code(), Some(REJECTED));
    assert!(text(&run.stderr).starts_with("error[L0506]: this file has no `main` function"));
}

#[test]
fn a_missing_c_compiler_is_explained() {
    let output = lymra_command(&["run", "examples/hello.lym"])
        .env("CC", "lymra-no-such-c-compiler")
        .output()
        .expect("the lymra executable runs");
    assert_eq!(output.status.code(), Some(FAILED));
    let report = text(&output.stderr);
    assert!(report.starts_with("error: no C compiler found"), "{report}");
    assert!(
        report.contains("install GCC or Clang, or set CC"),
        "{report}"
    );
}

#[test]
fn a_meaning_error_rejects_the_program() {
    let path = scratch_file("meaning.lym", b"function main()\n    total = 1\n");
    let output = lymra(&["check", &path.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(REJECTED));
    assert!(
        text(&output.stderr).starts_with("error[L0201]: assignment to undeclared name `total`")
    );
}
