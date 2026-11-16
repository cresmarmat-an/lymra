/*!
 * Random programs compiled to C and judged by the interpreter. Each is built
 * with every C compiler, with every check and optimized, and run; every
 * build has to print what the interpreter prints and end the same way.
 */

use std::num::NonZeroUsize;
use std::ops::AddAssign;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use lymra::codegen;
use lymra::diagnostics::render_report;
use lymra::driver::{Compiler, TemporaryDirectory, Warnings};
use lymra::frontend;
use lymra::ir::Program;
use lymra::optimize;

use super::common;
use super::interpreter::{self, Ending};

/// What a test counts in a program whose builds agreed: given the program as checked, the program optimized, and the optimized C.
pub type Measure<T> = fn(&Program<'_>, &Program<'_>, &str) -> T;

/// What became of one program.
enum Verdict<T> {
    /// The checker rejected it, which the generators are written to avoid. Holds the program and the report.
    Rejected(String),
    /// The interpreter ran out of steps, so nothing says what it should print.
    TooLong,
    /// Every build printed what the interpreter printed and ended the same way. Holds what the test measured.
    Agreed(T),
}

/// What became of a run of programs whose builds all agreed with the interpreter.
pub struct Tally<T> {
    /// How many programs ran to an end the interpreter knows.
    pub agreed: usize,
    /// What the test measured, added up over those programs.
    pub measured: T,
    /// How many programs ran out of the interpreter's steps.
    pub too_long: usize,
}

/// Checks, interprets, builds and runs one program. A disagreement comes back as an error that shows the program.
fn try_program<T>(
    name: &str,
    text: &str,
    compilers: &[Compiler],
    measure: Measure<T>,
) -> Result<Verdict<T>, String> {
    let source = common::source(name, text);
    let program = match frontend::check_program(&source) {
        Ok(program) => program,
        Err(diagnostics) => {
            let report = render_report(&diagnostics, &source);
            return Ok(Verdict::Rejected(format!("{text}\n{report}")));
        }
    };
    let main = program.main.expect("a checked program has `main`");
    let expected = interpreter::run(&program, main, &[])
        .map_err(|problem| format!("the interpreter found {problem} in:\n{text}"))?;
    let status = match expected.ending {
        Ending::Finished => 0,
        Ending::Trapped => 70,
        Ending::OutOfSteps => return Ok(Verdict::TooLong),
    };
    let every_check = codegen::generate(&program, &source);
    let mut optimized = program.clone();
    optimize::remove_proven_checks(&mut optimized);
    let fewer_checks = codegen::generate(&optimized, &source);
    for compiler in compilers {
        for (build, c_source) in [("every check", &every_check), ("optimized", &fewer_checks)] {
            let workspace = TemporaryDirectory::new().map_err(|error| error.to_string())?;
            let executable = workspace
                .path()
                .join(format!("program{}", std::env::consts::EXE_SUFFIX));
            compiler
                .compile(c_source, &executable, None, Warnings::Strict)
                .map_err(|error| {
                    format!("{} ({build}) rejects:\n{text}\n{error}", compiler.name())
                })?;
            let output = Command::new(&executable)
                .output()
                .map_err(|error| format!("cannot run a program: {error}"))?;
            let printed = String::from_utf8_lossy(&output.stdout);
            if output.status.code() != Some(status) || printed != expected.output {
                return Err(format!(
                    "{} ({build}) ended with {:?} where the interpreter expects {status}, \
                     printing:\n{printed}\ninstead of:\n{}\nerrors:\n{}\nin:\n{text}",
                    compiler.name(),
                    output.status.code(),
                    expected.output,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }
    Ok(Verdict::Agreed(measure(
        &program,
        &optimized,
        &fewer_checks,
    )))
}

/**
 * Runs every program in `texts`, as many at a time as the machine has
 * processors, each checked under the file name `name`. Panics with the
 * program on the first disagreement, and when the checker rejects any.
 */
pub fn run_all<T: Default + AddAssign + Send>(
    texts: &[String],
    name: &str,
    compilers: &[Compiler],
    measure: Measure<T>,
) -> Tally<T> {
    let workers = std::thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(texts.len());
    let next = AtomicUsize::new(0);
    let verdicts = Mutex::new(Vec::with_capacity(texts.len()));
    std::thread::scope(|threads| {
        for _ in 0..workers {
            threads.spawn(|| {
                while let Some(text) = texts.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let verdict = try_program(name, text, compilers, measure);
                    verdicts
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(verdict);
                }
            });
        }
    });
    let verdicts = verdicts
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    let mut tally = Tally {
        agreed: 0,
        measured: T::default(),
        too_long: 0,
    };
    let mut rejected = Vec::new();
    for verdict in verdicts {
        match verdict {
            Ok(Verdict::Agreed(measured)) => {
                tally.agreed += 1;
                tally.measured += measured;
            }
            Ok(Verdict::Rejected(report)) => rejected.push(report),
            Ok(Verdict::TooLong) => tally.too_long += 1,
            Err(failure) => panic!("{failure}"),
        }
    }
    if let Some(first) = rejected.first() {
        panic!(
            "the checker rejected {} generated programs, the first of them:\n{first}",
            rejected.len()
        );
    }
    tally
}
