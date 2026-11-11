//! The `lymra` command line: argument handling and the commands themselves.

use std::fs;
use std::io::Write;
use std::process::ExitCode;

use crate::ast::print_module;
use crate::diagnostics::{ErrorCode, render_report};
use crate::frontend::analyze;
use crate::source::SourceFile;

const SOURCE_EXTENSION: &str = ".lym";

const USAGE: &str = "\
Usage: lymra <command>

Commands:
  check <file.lym>     Check a file and report every error in it
  parse <file.lym>     Print the syntax tree of a file
  explain <code>       Explain an error code, for example: lymra explain L0102
  help                 Show this message
  version              Show the compiler version
";

/// How a run of the command line ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    /// The Lymra program has errors. They were reported.
    Rejected,
    /// The command line itself was wrong, or a file could not be read.
    Failed,
}

impl From<Outcome> for ExitCode {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Success => ExitCode::SUCCESS,
            Outcome::Rejected => ExitCode::from(1),
            Outcome::Failed => ExitCode::from(2),
        }
    }
}

/// Runs the command line. Results go to `stdout`; errors and diagnostics go to `stderr`.
pub fn run(arguments: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Outcome {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["check", path] => check(path, stdout, stderr),
        ["parse", path] => parse(path, stdout, stderr),
        ["explain", code] => explain(code, stdout, stderr),
        ["help" | "--help" | "-h"] => print(stdout, USAGE, Outcome::Success),
        ["version" | "--version" | "-V"] => print(
            stdout,
            &format!("lymra {}\n", env!("CARGO_PKG_VERSION")),
            Outcome::Success,
        ),
        [] => print(stderr, USAGE, Outcome::Failed),
        [command @ ("check" | "parse" | "explain")] => {
            let operand = if *command == "explain" {
                "an error code"
            } else {
                "a file"
            };
            let message = format!("error: `lymra {command}` needs {operand}\n\n{USAGE}");
            print(stderr, &message, Outcome::Failed)
        }
        [command @ ("check" | "parse" | "explain"), ..] => {
            let message = format!("error: `lymra {command}` takes exactly one argument\n\n{USAGE}");
            print(stderr, &message, Outcome::Failed)
        }
        [command, ..] => {
            let message = format!("error: unknown command `{command}`\n\n{USAGE}");
            print(stderr, &message, Outcome::Failed)
        }
    }
}

fn check(path: &str, stdout: &mut dyn Write, stderr: &mut dyn Write) -> Outcome {
    let source = match load(path) {
        Ok(source) => source,
        Err(message) => return print(stderr, &message, Outcome::Failed),
    };
    let analysis = analyze(&source);
    if analysis.diagnostics.is_empty() {
        return print(stdout, &format!("{path}: no errors\n"), Outcome::Success);
    }
    print(
        stderr,
        &render_report(&analysis.diagnostics, &source),
        Outcome::Rejected,
    )
}

fn parse(path: &str, stdout: &mut dyn Write, stderr: &mut dyn Write) -> Outcome {
    let source = match load(path) {
        Ok(source) => source,
        Err(message) => return print(stderr, &message, Outcome::Failed),
    };
    let analysis = analyze(&source);
    if analysis.diagnostics.is_empty() {
        return print(stdout, &print_module(&analysis.module), Outcome::Success);
    }
    print(
        stderr,
        &render_report(&analysis.diagnostics, &source),
        Outcome::Rejected,
    )
}

fn explain(code: &str, stdout: &mut dyn Write, stderr: &mut dyn Write) -> Outcome {
    match ErrorCode::from_text(code) {
        Some(code) => print(stdout, code.explanation(), Outcome::Success),
        None => {
            let message = format!(
                "error: `{code}` is not an error code of this compiler\n\
                 help: codes look like `L0102`; every error message starts with its code\n"
            );
            print(stderr, &message, Outcome::Failed)
        }
    }
}

/// Reads a source file. The error is a complete message, ready to print.
fn load(path: &str) -> Result<SourceFile, String> {
    if !path.ends_with(SOURCE_EXTENSION) {
        return Err(format!(
            "error: `{path}` is not a Lymra source file\nhelp: Lymra source files end in `{SOURCE_EXTENSION}`\n"
        ));
    }
    let bytes =
        fs::read(path).map_err(|error| format!("error: cannot read `{path}`: {error}\n"))?;
    let text = String::from_utf8(bytes).map_err(|error| not_utf8(path, &error))?;
    SourceFile::new(path, text).map_err(|_| format!("error: `{path}` is larger than 4 GiB\n"))
}

/// Reports a file that is not UTF-8. No source line can be shown, because the text cannot be decoded.
fn not_utf8(path: &str, error: &std::string::FromUtf8Error) -> String {
    let valid_bytes = error.utf8_error().valid_up_to();
    let valid_prefix = error.as_bytes().get(..valid_bytes).unwrap_or_default();
    let line = valid_prefix.iter().filter(|&&byte| byte == b'\n').count() + 1;
    format!(
        "error[{code}]: `{path}` is not valid UTF-8\n  --> {path}:{line}\n\
         help: save the file with UTF-8 encoding; the first invalid byte is on line {line}\n",
        code = ErrorCode::L0115
    )
}

/// Writes `text` and passes `outcome` through, so a command can end with one expression.
fn print(stream: &mut dyn Write, text: &str, outcome: Outcome) -> Outcome {
    // A closed pipe is not worth reporting: the reader has gone, and there is nowhere left to say so.
    let _ = stream.write_all(text.as_bytes());
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the command line and returns the outcome with what was written to each stream.
    fn run_with(arguments: &[&str]) -> (Outcome, String, String) {
        let arguments: Vec<String> = arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let outcome = run(&arguments, &mut stdout, &mut stderr);
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).expect("output is UTF-8");
        (outcome, text(stdout), text(stderr))
    }

    #[test]
    fn help_and_version_succeed() {
        let (outcome, stdout, stderr) = run_with(&["help"]);
        assert_eq!((outcome, stderr.as_str()), (Outcome::Success, ""));
        assert!(stdout.starts_with("Usage: lymra <command>"));

        let (outcome, stdout, _) = run_with(&["--version"]);
        assert_eq!(outcome, Outcome::Success);
        assert_eq!(stdout, format!("lymra {}\n", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn a_wrong_command_line_fails_with_the_usage() {
        for arguments in [
            &[][..],
            &["compile"],
            &["check"],
            &["check", "a.lym", "b.lym"],
        ] {
            let (outcome, stdout, stderr) = run_with(arguments);
            assert_eq!(
                (outcome, stdout.as_str()),
                (Outcome::Failed, ""),
                "for {arguments:?}"
            );
            assert!(
                stderr.contains("Usage: lymra <command>"),
                "for {arguments:?}"
            );
        }
    }

    #[test]
    fn explain_prints_the_page_for_a_code() {
        let (outcome, stdout, _) = run_with(&["explain", "l0102"]);
        assert_eq!(outcome, Outcome::Success);
        assert_eq!(stdout, ErrorCode::L0102.explanation());

        let (outcome, _, stderr) = run_with(&["explain", "E0308"]);
        assert_eq!(outcome, Outcome::Failed);
        assert!(stderr.contains("`E0308` is not an error code of this compiler"));
    }

    #[test]
    fn files_that_cannot_be_loaded_fail_before_any_analysis() {
        let (outcome, _, stderr) = run_with(&["check", "notes.txt"]);
        assert_eq!(outcome, Outcome::Failed);
        assert!(stderr.contains("Lymra source files end in `.lym`"));

        let (outcome, _, stderr) = run_with(&["check", "no/such/file.lym"]);
        assert_eq!(outcome, Outcome::Failed);
        assert!(stderr.starts_with("error: cannot read `no/such/file.lym`"));
    }
}
