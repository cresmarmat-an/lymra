//! The `lymra` command line: argument handling and the commands themselves.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use crate::ast::print_module;
use crate::codegen;
use crate::diagnostics::{ErrorCode, render_report};
use crate::driver::{self, Compiler, DriverError, TemporaryDirectory, Warnings};
use crate::frontend::{self, analyze};
use crate::ir::Program;
use crate::optimize;
use crate::source::SourceFile;

const SOURCE_EXTENSION: &str = ".lym";

const USAGE: &str = "\
Usage: lymra <command>

Commands:
  run <file.lym>       Build a program and run it
  build <file.lym>     Build a program into an executable next to its source
  check <file.lym>     Check a file and report every error in it
  emit <file.lym>      Print the C code the compiler generates for a program
  parse <file.lym>     Print the syntax tree of a file
  explain <code>       Explain an error code, for example: lymra explain L0102
  help                 Show this message
  version              Show the compiler version

Building needs a C compiler: GCC, Clang or MSVC. Set CC to choose one.
";

/// The commands that take exactly one operand: a file, or an error code for `explain`.
const COMMANDS_WITH_OPERAND: [&str; 6] = ["run", "build", "check", "emit", "parse", "explain"];

/// How a run of the command line ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    /// The Lymra program has errors. They were reported.
    Rejected,
    /// The command line itself was wrong, a file could not be read, or no C compiler was found.
    Failed,
    /// `lymra run` started the program, and this is the program's own exit code.
    Exited(u8),
}

impl From<Outcome> for ExitCode {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Success => ExitCode::SUCCESS,
            Outcome::Rejected => ExitCode::from(1),
            Outcome::Failed => ExitCode::from(2),
            Outcome::Exited(code) => ExitCode::from(code),
        }
    }
}

/// Runs the command line. Results go to `stdout`; errors and diagnostics go to `stderr`.
pub fn run(arguments: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Outcome {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["run", path] => run_program(path, stdout, stderr),
        ["build", path] => build(path, stdout, stderr),
        ["check", path] => check(path, stdout, stderr),
        ["emit", path] => emit(path, stdout, stderr),
        ["parse", path] => parse(path, stdout, stderr),
        ["explain", code] => explain(code, stdout, stderr),
        ["help" | "--help" | "-h"] => print(stdout, USAGE, Outcome::Success),
        ["version" | "--version" | "-V"] => print(
            stdout,
            &format!("lymra {}\n", env!("CARGO_PKG_VERSION")),
            Outcome::Success,
        ),
        [] => print(stderr, USAGE, Outcome::Failed),
        [command] if COMMANDS_WITH_OPERAND.contains(command) => {
            let operand = if *command == "explain" {
                "an error code"
            } else {
                "a file"
            };
            let message = format!("error: `lymra {command}` needs {operand}\n\n{USAGE}");
            print(stderr, &message, Outcome::Failed)
        }
        [command, ..] if COMMANDS_WITH_OPERAND.contains(command) => {
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
    let source = match load_or_report(path, stderr) {
        Ok(source) => source,
        Err(outcome) => return outcome,
    };
    match frontend::check(&source) {
        Ok(_) => print(stdout, &format!("{path}: no errors\n"), Outcome::Success),
        Err(diagnostics) => print(
            stderr,
            &render_report(&diagnostics, &source),
            Outcome::Rejected,
        ),
    }
}

fn emit(path: &str, stdout: &mut dyn Write, stderr: &mut dyn Write) -> Outcome {
    let source = match load_or_report(path, stderr) {
        Ok(source) => source,
        Err(outcome) => return outcome,
    };
    match checked_program(&source, stderr) {
        Ok(program) => print(
            stdout,
            &codegen::generate(&program, &source),
            Outcome::Success,
        ),
        Err(outcome) => outcome,
    }
}

fn build(path: &str, stdout: &mut dyn Write, stderr: &mut dyn Write) -> Outcome {
    let source = match load_or_report(path, stderr) {
        Ok(source) => source,
        Err(outcome) => return outcome,
    };
    let program = match checked_program(&source, stderr) {
        Ok(program) => program,
        Err(outcome) => return outcome,
    };
    let executable = driver::executable_path(Path::new(path));
    match compile(&program, &source, &executable) {
        Ok(()) => print(
            stdout,
            &format!("{path}: built {}\n", executable.display()),
            Outcome::Success,
        ),
        Err(error) => print(stderr, &error.to_string(), Outcome::Failed),
    }
}

/// Builds the program in a temporary directory, runs it, and passes its exit code on.
fn run_program(path: &str, stdout: &mut dyn Write, stderr: &mut dyn Write) -> Outcome {
    let source = match load_or_report(path, stderr) {
        Ok(source) => source,
        Err(outcome) => return outcome,
    };
    let program = match checked_program(&source, stderr) {
        Ok(program) => program,
        Err(outcome) => return outcome,
    };
    let workspace = match TemporaryDirectory::new() {
        Ok(workspace) => workspace,
        Err(error) => return print(stderr, &error.to_string(), Outcome::Failed),
    };
    let executable = workspace
        .path()
        .join(format!("program{}", std::env::consts::EXE_SUFFIX));
    if let Err(error) = compile(&program, &source, &executable) {
        return print(stderr, &error.to_string(), Outcome::Failed);
    }
    // Anything this process still holds must be written before the program writes its own output.
    let _ = stdout.flush();
    match driver::run_executable(&executable) {
        Ok(status) => Outcome::Exited(driver::exit_code(status)),
        Err(error) => print(stderr, &error.to_string(), Outcome::Failed),
    }
}

fn compile(
    program: &Program<'_>,
    source: &SourceFile,
    executable: &Path,
) -> Result<(), DriverError> {
    let compiler = Compiler::find()?;
    let c_source = codegen::generate(program, source);
    compiler.compile(&c_source, executable, program.optimize, Warnings::Default)
}

/**
 * Checks a program that is meant to run, and removes the runtime checks it
 * proves can never fail. Errors are written to `stderr` before this returns.
 */
fn checked_program<'src>(
    source: &'src SourceFile,
    stderr: &mut dyn Write,
) -> Result<Program<'src>, Outcome> {
    let mut program = frontend::check_program(source).map_err(|diagnostics| {
        print(
            stderr,
            &render_report(&diagnostics, source),
            Outcome::Rejected,
        )
    })?;
    optimize::remove_proven_checks(&mut program);
    Ok(program)
}

fn parse(path: &str, stdout: &mut dyn Write, stderr: &mut dyn Write) -> Outcome {
    let source = match load_or_report(path, stderr) {
        Ok(source) => source,
        Err(outcome) => return outcome,
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

/// Reads a source file. A failure is written to `stderr` before this returns.
fn load_or_report(path: &str, stderr: &mut dyn Write) -> Result<SourceFile, Outcome> {
    load(path).map_err(|message| print(stderr, &message, Outcome::Failed))
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
