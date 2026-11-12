//! The ways each program is built: as C, as Lymra with and without the optimizer, and as Rust.

use std::fmt;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use lymra::codegen;
use lymra::diagnostics::render_report;
use lymra::driver::{Compiler, Warnings};
use lymra::frontend;
use lymra::optimize;
use lymra::source::SourceFile;

/// The C compilers to try. Each one builds both the C and the Lymra versions.
const C_COMPILERS: [&str; 2] = ["gcc", "clang"];
/// What `lymra build` passes to the C compiler by default. The C versions get the same.
pub const C_FLAGS: [&str; 2] = ["-std=c11", "-O2"];
/// Rust's usual release settings. Overflow checks are off unless a variant turns them on.
pub const RUST_FLAGS: [&str; 4] = ["--edition", "2024", "-C", "opt-level=3"];

/// One way of building a program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// The C version, which checks nothing.
    C { compiler: &'static str },
    /// Lymra as `lymra build` makes it: the checks the optimizer proves can never fail are gone.
    Lymra { compiler: &'static str },
    /// Lymra with every check, as if there were no optimizer.
    LymraEveryCheck { compiler: &'static str },
    /// The Rust version with release settings, where overflow wraps around silently.
    Rust,
    /// The Rust version with overflow checks, which stop the program the way Lymra's do.
    RustChecked,
}

impl fmt::Display for Variant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::C { compiler } => write!(formatter, "C ({compiler})"),
            Self::Lymra { compiler } => write!(formatter, "Lymra ({compiler})"),
            Self::LymraEveryCheck { compiler } => {
                write!(formatter, "Lymra with every check ({compiler})")
            }
            Self::Rust => formatter.write_str("Rust"),
            Self::RustChecked => formatter.write_str("Rust with overflow checks"),
        }
    }
}

impl Variant {
    /// Builds the program in `directory` into `executable`.
    pub fn build(self, directory: &Path, executable: &Path) -> Result<(), String> {
        match self {
            Self::C { compiler } => run_tool(
                Command::new(compiler)
                    .args(C_FLAGS)
                    .arg("-o")
                    .arg(executable)
                    .arg(directory.join("program.c")),
            ),
            Self::Lymra { compiler } => build_lymra(directory, executable, compiler, true),
            Self::LymraEveryCheck { compiler } => {
                build_lymra(directory, executable, compiler, false)
            }
            Self::Rust => build_rust(directory, executable, false),
            Self::RustChecked => build_rust(directory, executable, true),
        }
    }
}

/// Every variant whose compiler is installed, with each C version before the Lymra versions built by the same compiler.
pub fn available() -> Vec<Variant> {
    let mut variants = Vec::new();
    for compiler in C_COMPILERS {
        if Compiler::named(compiler).is_available() {
            variants.extend([
                Variant::C { compiler },
                Variant::Lymra { compiler },
                Variant::LymraEveryCheck { compiler },
            ]);
        } else {
            eprintln!("note: `{compiler}` is not installed, so the results leave it out");
        }
    }
    if Command::new("rustc").arg("--version").output().is_ok() {
        variants.extend([Variant::Rust, Variant::RustChecked]);
    } else {
        eprintln!("note: `rustc` is not installed, so the results leave Rust out");
    }
    variants
}

fn build_lymra(
    directory: &Path,
    executable: &Path,
    compiler: &str,
    remove_proven_checks: bool,
) -> Result<(), String> {
    let path = directory.join("program.lym");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let source = SourceFile::new(path.display().to_string(), text)
        .map_err(|_| format!("{} is too large", path.display()))?;
    let mut program = frontend::check_program(&source)
        .map_err(|diagnostics| render_report(&diagnostics, &source))?;
    if remove_proven_checks {
        optimize::remove_proven_checks(&mut program);
    }
    let c_source = codegen::generate(&program, &source);
    Compiler::named(compiler)
        .compile(&c_source, executable, program.optimize, Warnings::Default)
        .map_err(|error| error.to_string())
}

fn build_rust(directory: &Path, executable: &Path, overflow_checks: bool) -> Result<(), String> {
    let mut command = Command::new("rustc");
    command.args(RUST_FLAGS);
    if overflow_checks {
        command.args(["-C", "overflow-checks=on"]);
    }
    run_tool(
        command
            .arg("-o")
            .arg(executable)
            .arg(directory.join("program.rs")),
    )
}

/// Runs a compiler and turns a failure into its message.
fn run_tool(command: &mut Command) -> Result<(), String> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("cannot run {command:?}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{command:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    ))
}
