/*!
 * The driver: compiles generated C with the system's C compiler, and runs the result.
 *
 * Lymra uses GCC or Clang as its back end, so its programs get the speed of
 * mature optimizers without a code generator of its own. The C compiler is
 * the one named by the `CC` environment variable, or else the first of `cc`,
 * `gcc` and `clang` that can be started.
 */

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ast::OptimizeMode;

const COMPILER_CANDIDATES: [&str; 3] = ["cc", "gcc", "clang"];
const C_STANDARD: &str = "-std=c11";

/// How the C compiler treats warnings in the generated code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warnings {
    /// The compiler's defaults. Warnings never stop a build.
    Default,
    /// Every warning is an error. The test suite uses this to keep the generated C clean.
    Strict,
}

#[derive(Debug)]
pub enum DriverError {
    /// No C compiler could be started.
    NoCompiler { tried: Vec<String> },
    /// The C compiler rejected the generated code, which is a bug in this compiler.
    CompilerRejected { command: String, output: String },
    /// A file, a directory or a process could not be used.
    Io { action: String, error: io::Error },
}

impl fmt::Display for DriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCompiler { tried } => write!(
                formatter,
                "error: no C compiler found (tried {})\n\
                 help: Lymra compiles to C; install GCC or Clang, or set CC to the compiler to use\n",
                tried.join(", ")
            ),
            Self::CompilerRejected { command, output } => write!(
                formatter,
                "error: the C compiler rejected the code Lymra generated\n\
                 This is a bug in the Lymra compiler, not in your program. Please report it with the source file.\n\
                 command: {command}\n{output}"
            ),
            Self::Io { action, error } => writeln!(formatter, "error: cannot {action}: {error}"),
        }
    }
}

fn io_error(action: impl Into<String>) -> impl FnOnce(io::Error) -> DriverError {
    let action = action.into();
    move |error| DriverError::Io { action, error }
}

/// A C compiler and the arguments that always come first, as `CC` may hold both.
#[derive(Clone, Debug)]
pub struct Compiler {
    program: OsString,
    leading_arguments: Vec<OsString>,
}

impl Compiler {
    /// The compiler named by `CC`, or the first of `cc`, `gcc` and `clang` that can be started.
    pub fn find() -> Result<Self, DriverError> {
        if let Some(variable) = std::env::var_os("CC").filter(|value| !value.is_empty()) {
            let text = variable.to_string_lossy().into_owned();
            let mut words = text.split_whitespace().map(OsString::from);
            if let Some(program) = words.next() {
                return Ok(Self {
                    program,
                    leading_arguments: words.collect(),
                });
            }
        }
        COMPILER_CANDIDATES
            .iter()
            .map(|name| Self::named(name))
            .find(Self::is_available)
            .ok_or_else(|| DriverError::NoCompiler {
                tried: COMPILER_CANDIDATES
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect(),
            })
    }

    /// A compiler by name, for callers that want a specific one.
    pub fn named(program: &str) -> Self {
        Self {
            program: OsString::from(program),
            leading_arguments: Vec::new(),
        }
    }

    pub fn name(&self) -> String {
        self.program.to_string_lossy().into_owned()
    }

    /// True if the compiler can be started at all.
    pub fn is_available(&self) -> bool {
        Command::new(&self.program)
            .args(&self.leading_arguments)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }

    /// Compiles C source into an executable at `executable`.
    pub fn compile(
        &self,
        c_source: &str,
        executable: &Path,
        optimize: Option<OptimizeMode>,
        warnings: Warnings,
    ) -> Result<(), DriverError> {
        let workspace = TemporaryDirectory::new()?;
        let c_file = workspace.path().join("program.c");
        fs::write(&c_file, c_source).map_err(io_error(format!("write {}", c_file.display())))?;

        let mut command = Command::new(&self.program);
        command
            .args(&self.leading_arguments)
            .arg(C_STANDARD)
            .args(optimization_flags(optimize));
        if warnings == Warnings::Strict {
            command.args(["-Wall", "-Wextra", "-Wpedantic", "-Werror"]);
        }
        command.arg("-o").arg(executable).arg(&c_file);

        let output = match command.stdin(Stdio::null()).output() {
            Ok(output) => output,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(DriverError::NoCompiler {
                    tried: vec![format!("`{}`", self.name())],
                });
            }
            Err(error) => return Err(io_error(format!("run {}", self.name()))(error)),
        };
        if output.status.success() {
            return Ok(());
        }
        let mut report = String::from_utf8_lossy(&output.stderr).into_owned();
        report.push_str(&String::from_utf8_lossy(&output.stdout));
        Err(DriverError::CompilerRejected {
            command: format!("{command:?}"),
            output: report,
        })
    }
}

fn optimization_flags(mode: Option<OptimizeMode>) -> &'static [&'static str] {
    match mode {
        None | Some(OptimizeMode::Speed) => &["-O2"],
        Some(OptimizeMode::Size) => &["-Os"],
        Some(OptimizeMode::Debug) => &["-O0", "-g"],
    }
}

/// Where `lymra build` puts the executable: next to the source, named after it.
pub fn executable_path(source: &Path) -> PathBuf {
    let mut executable = source.with_extension("");
    if !std::env::consts::EXE_SUFFIX.is_empty() {
        executable.set_extension(std::env::consts::EXE_EXTENSION);
    }
    executable
}

/// Runs an executable with this process's input and output, and waits for it.
pub fn run_executable(executable: &Path) -> Result<ExitStatus, DriverError> {
    Command::new(executable)
        .status()
        .map_err(io_error(format!("run {}", executable.display())))
}

/// The exit code to pass on for a finished program. A program killed by signal `n` gives 128 + n, as shells do.
pub fn exit_code(status: ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        // Exit codes are one byte on every platform Lymra builds for; the mask makes the cast exact.
        return (code & 0xFF) as u8;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128_u8.saturating_add((signal & 0x7F) as u8);
        }
    }
    1
}

/// A directory that is removed with everything in it when this value is dropped.
pub struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    pub fn new() -> Result<Self, DriverError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanoseconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("lymra-{}-{nanoseconds}-{sequence}", std::process::id());
        let path = std::env::temp_dir().join(name);
        fs::create_dir(&path).map_err(io_error(format!("create {}", path.display())))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        // Nothing useful can be done if the cleanup fails; the system clears its temporary directory eventually.
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_executable_is_named_after_the_source() {
        let executable = executable_path(Path::new("examples/hello.lym"));
        let expected = format!("examples/hello{}", std::env::consts::EXE_SUFFIX);
        assert_eq!(executable, PathBuf::from(expected));
    }

    #[test]
    fn optimization_follows_the_directive() {
        assert_eq!(optimization_flags(None), ["-O2"]);
        assert_eq!(optimization_flags(Some(OptimizeMode::Size)), ["-Os"]);
        assert_eq!(optimization_flags(Some(OptimizeMode::Debug)), ["-O0", "-g"]);
    }

    #[test]
    fn a_temporary_directory_is_removed_when_dropped() {
        let directory = TemporaryDirectory::new().expect("the temporary directory can be created");
        let path = directory.path().to_path_buf();
        fs::write(path.join("file"), "contents").expect("a file can be written inside");
        drop(directory);
        assert!(!path.exists());
    }

    #[test]
    fn a_missing_compiler_is_reported_as_missing() {
        let compiler = Compiler::named("lymra-no-such-c-compiler");
        assert!(!compiler.is_available());
        let directory = TemporaryDirectory::new().expect("the temporary directory can be created");
        let result = compiler.compile(
            "int main(void) { return 0; }",
            &directory.path().join("out"),
            None,
            Warnings::Default,
        );
        assert!(matches!(result, Err(DriverError::NoCompiler { .. })));
    }
}
