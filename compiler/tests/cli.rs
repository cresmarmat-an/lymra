//! Runs the real `lymra` executable, to check exit codes and where output goes.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SUCCESS: i32 = 0;
const REJECTED: i32 = 1;
const FAILED: i32 = 2;

/// Runs `lymra` from the repository root, so paths in its output are the ones a user would type.
fn lymra(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lymra"))
        .args(arguments)
        .current_dir(common::repository_root())
        .output()
        .expect("the lymra executable runs")
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
