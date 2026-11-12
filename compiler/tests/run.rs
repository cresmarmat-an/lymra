/*!
 * Programs that compile and run.
 *
 * Every `.lym` file under `tests/run` and in `examples` is checked, turned
 * into C, compiled with each C compiler on this machine with all warnings as
 * errors, and run. Its exit status, output and error output have to match
 * the `.run` file recorded for it.
 *
 * Each program is built twice: with every runtime check, and with the checks
 * the optimizer proves unnecessary removed. Every build, with every compiler,
 * has to give the same result, so a check removed by mistake shows up here.
 */

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use lymra::codegen;
use lymra::diagnostics::render_report;
use lymra::driver::{Compiler, TemporaryDirectory, Warnings};
use lymra::frontend;
use lymra::optimize;

const COMPILERS: [&str; 2] = ["gcc", "clang"];

/// A program to run, the name it is reported under, and where its recorded result lives.
struct Case {
    source: PathBuf,
    name: String,
    golden: PathBuf,
}

fn cases() -> Vec<Case> {
    let compiler_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let run_directory = compiler_root.join("tests/run");
    let tests = common::files_with_extension(&run_directory, "lym")
        .into_iter()
        .filter(|path| path.parent() == Some(run_directory.as_path()))
        .map(|source| Case {
            name: format!("tests/run/{}", file_name(&source)),
            golden: source.with_extension("run"),
            source,
        });
    let examples = common::files_with_extension(&common::repository_root().join("examples"), "lym")
        .into_iter()
        .map(|source| Case {
            name: format!("examples/{}", file_name(&source)),
            golden: run_directory
                .join("examples")
                .join(file_name(&source))
                .with_extension("run"),
            source,
        });
    tests.chain(examples).collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The recorded form of one run: exit status, then standard output, then standard error.
fn describe_run(status: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    let status = status.map_or("killed by a signal".to_owned(), |code| code.to_string());
    format!(
        "exit status {status}\n--- standard output\n{}--- standard error\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    )
}

/// Builds and runs one program with every compiler. Returns the result they agree on.
fn run_case(case: &Case, compilers: &[Compiler]) -> Result<String, String> {
    let source = common::source(&case.name, &common::read(&case.source));
    let program = frontend::check_program(&source).map_err(|diagnostics| {
        format!(
            "{} does not check:\n{}",
            case.name,
            render_report(&diagnostics, &source)
        )
    })?;
    let every_check = codegen::generate(&program, &source);
    let mut optimized = program;
    optimize::remove_proven_checks(&mut optimized);
    let fewer_checks = codegen::generate(&optimized, &source);
    let builds = [("every check", &every_check), ("optimized", &fewer_checks)];

    let mut agreed: Option<(String, String)> = None;
    for (compiler, (build, c_source)) in compilers
        .iter()
        .flat_map(|compiler| builds.iter().map(move |build| (compiler, build)))
    {
        let label = format!("{} ({build})", compiler.name());
        let workspace = TemporaryDirectory::new().map_err(|error| error.to_string())?;
        let executable = workspace
            .path()
            .join(format!("program{}", std::env::consts::EXE_SUFFIX));
        compiler
            .compile(c_source, &executable, optimized.optimize, Warnings::Strict)
            .map_err(|error| format!("{} with {label}:\n{error}", case.name))?;
        let output = Command::new(&executable)
            .output()
            .map_err(|error| format!("{}: cannot run the program: {error}", case.name))?;
        let result = describe_run(output.status.code(), &output.stdout, &output.stderr);
        match &agreed {
            Some((first_label, first_result)) if *first_result != result => {
                return Err(format!(
                    "{}: {first_label} and {label} disagree\n--- {first_label}:\n{first_result}\n--- {label}:\n{result}",
                    case.name
                ));
            }
            Some(_) => {}
            None => agreed = Some((label, result)),
        }
    }
    agreed
        .map(|(_, result)| result)
        .ok_or_else(|| "no compiler ran".to_owned())
}

#[test]
fn every_program_runs_as_recorded() {
    let compilers: Vec<Compiler> = COMPILERS
        .iter()
        .map(|name| Compiler::named(name))
        .filter(Compiler::is_available)
        .collect();
    assert!(
        !compilers.is_empty(),
        "these tests need a C compiler; install GCC or Clang"
    );
    let cases = cases();
    assert!(!cases.is_empty(), "no programs found under tests/run");

    let results: Vec<Result<String, String>> = std::thread::scope(|threads| {
        let handles: Vec<_> = cases
            .iter()
            .map(|case| threads.spawn(|| run_case(case, &compilers)))
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("the test thread panicked".to_owned()))
            })
            .collect()
    });

    let mut mismatches = Vec::new();
    for (case, result) in cases.iter().zip(results) {
        match result {
            Ok(actual) => mismatches.extend(common::check_golden(&case.golden, Some(&actual))),
            Err(failure) => mismatches.push(failure),
        }
    }
    common::assert_no_mismatches(&mismatches);
}
