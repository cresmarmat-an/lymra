/*!
 * The driver: compiles generated C with the system's C compiler, and runs the result.
 *
 * Lymra uses GCC, Clang or MSVC as its back end, so its programs get the
 * speed of mature optimizers without a code generator of its own. The C
 * compiler is the one named by the `CC` environment variable, or else the
 * first of `cc`, `gcc`, `clang` and `cl` that can be started.
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

const COMPILER_CANDIDATES: [&str; 4] = ["cc", "gcc", "clang", "cl"];

/// The first MSVC with C11, which the generated code is written in: the one in Visual Studio 2019 version 16.8.
const OLDEST_MSVC: (u32, u32) = (19, 28);

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
    /// The C compiler is an MSVC older than the generated code needs.
    CompilerTooOld { compiler: String, version: String },
    /// A file, a directory or a process could not be used.
    Io { action: String, error: io::Error },
}

impl fmt::Display for DriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCompiler { tried } => write!(
                formatter,
                "error: no C compiler found (tried {})\n\
                 help: Lymra compiles to C; install GCC, Clang or MSVC, or set CC to the compiler to use. \
                 MSVC's `cl` is found from a Visual Studio developer prompt.\n",
                tried.join(", ")
            ),
            Self::CompilerRejected { command, output } => write!(
                formatter,
                "error: the C compiler rejected the code Lymra generated\n\
                 This is a bug in the Lymra compiler, not in your program. Please report it with the source file.\n\
                 command: {command}\n{output}"
            ),
            Self::CompilerTooOld { compiler, version } => write!(
                formatter,
                "error: `{compiler}` is MSVC {version}, too old to build Lymra programs\n\
                 help: Lymra needs MSVC {}.{} or newer, from Visual Studio 2019 version 16.8 on; \
                 update Visual Studio or its Build Tools, or set CC to GCC or Clang\n",
                OLDEST_MSVC.0, OLDEST_MSVC.1
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiler {
    program: OsString,
    leading_arguments: Vec<OsString>,
}

/// How a C compiler takes its options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    /// GCC, Clang, and the compilers that take the same options, such as `cc`.
    Gnu,
    /// MSVC's `cl` and Clang's `clang-cl`, which take Windows-style options.
    Msvc,
}

impl Compiler {
    /// The compiler named by `CC`, or the first of `cc`, `gcc`, `clang` and `cl` that can be started.
    pub fn find() -> Result<Self, DriverError> {
        if let Some(chosen) = Self::from_environment() {
            return Ok(chosen);
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

    /// The compiler `CC` names with its leading arguments, as in `CC="gcc -m32"`, or `None` when `CC` is not set.
    pub fn from_environment() -> Option<Self> {
        let variable = std::env::var_os("CC")?;
        Self::from_command(&variable.to_string_lossy())
    }

    /// The compiler a command starts with and the arguments after it, as in `gcc -m32`, or `None` for a blank command.
    pub fn from_command(command: &str) -> Option<Self> {
        let mut words = command.split_whitespace().map(OsString::from);
        let program = words.next()?;
        Some(Self {
            program,
            leading_arguments: words.collect(),
        })
    }

    /// A compiler by name, for callers that want a specific one.
    pub fn named(program: &str) -> Self {
        Self {
            program: OsString::from(program),
            leading_arguments: Vec::new(),
        }
    }

    /// The compiler as it would be written in `CC`: the program and its leading arguments.
    pub fn name(&self) -> String {
        std::iter::once(&self.program)
            .chain(&self.leading_arguments)
            .map(|word| word.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn style(&self) -> Style {
        let stem = Path::new(&self.program)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_ascii_lowercase());
        match stem.as_deref() {
            Some("cl" | "clang-cl") => Style::Msvc,
            _ => Style::Gnu,
        }
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
        command.args(&self.leading_arguments);
        match self.style() {
            Style::Gnu => {
                command.arg("-std=c11").args(optimization_flags(optimize));
                if warnings == Warnings::Strict {
                    command.args(["-Wall", "-Wextra", "-Wpedantic", "-Werror"]);
                }
                command.arg("-o").arg(executable).arg(&c_file);
            }
            Style::Msvc => {
                // MSVC leaves object files in its working directory, so that is the temporary one.
                let executable = std::path::absolute(executable)
                    .map_err(io_error(format!("find {}", executable.display())))?;
                let mut output = OsString::from("/Fe:");
                output.push(executable.as_os_str());
                command
                    .current_dir(workspace.path())
                    .args(["/nologo", "/std:c11", "/utf-8"])
                    .args(msvc_optimization_flags(optimize));
                if warnings == Warnings::Strict {
                    command.args(["/W4", "/WX"]);
                }
                command.arg(output).arg(&c_file);
            }
        }

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
        if let Some(version) = self.too_old_msvc() {
            return Err(DriverError::CompilerTooOld {
                compiler: self.name(),
                version,
            });
        }
        let mut report = String::from_utf8_lossy(&output.stderr).into_owned();
        report.push_str(&String::from_utf8_lossy(&output.stdout));
        Err(DriverError::CompilerRejected {
            command: format!("{command:?}"),
            output: report,
        })
    }

    /**
     * The version of this compiler when it is an MSVC too old for the
     * generated code. Asked only after the compiler rejected the code, so a
     * successful build never pays for it.
     */
    fn too_old_msvc(&self) -> Option<String> {
        if self.style() != Style::Msvc {
            return None;
        }
        let output = Command::new(&self.program)
            .args(&self.leading_arguments)
            .stdin(Stdio::null())
            .output()
            .ok()?;
        let version = msvc_version(&String::from_utf8_lossy(&output.stderr))?;
        (version < OLDEST_MSVC).then(|| format!("{}.{}", version.0, version.1))
    }
}

/// The major and minor version in the banner `cl` prints, as in `Version 19.29.30154`, in whatever language it is in.
fn msvc_version(banner: &str) -> Option<(u32, u32)> {
    banner.split_whitespace().find_map(|word| {
        let mut numbers = word.split('.').map(str::parse::<u32>);
        let major = numbers.next()?.ok()?;
        let minor = numbers.next()?.ok()?;
        // A version has a third number, which keeps other numbers in the banner from being taken for one.
        numbers.next()?.ok()?;
        Some((major, minor))
    })
}

fn optimization_flags(mode: Option<OptimizeMode>) -> &'static [&'static str] {
    match mode {
        None | Some(OptimizeMode::Speed) => &["-O2"],
        Some(OptimizeMode::Size) => &["-Os"],
        Some(OptimizeMode::Debug) => &["-O0", "-g"],
    }
}

/// The same choices in MSVC's spelling.
fn msvc_optimization_flags(mode: Option<OptimizeMode>) -> &'static [&'static str] {
    match mode {
        None | Some(OptimizeMode::Speed) => &["/O2"],
        Some(OptimizeMode::Size) => &["/O1"],
        Some(OptimizeMode::Debug) => &["/Od", "/Zi"],
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
        assert_eq!(msvc_optimization_flags(None), ["/O2"]);
        assert_eq!(msvc_optimization_flags(Some(OptimizeMode::Size)), ["/O1"]);
    }

    #[test]
    fn a_command_names_the_compiler_and_its_leading_arguments() {
        let compiler =
            Compiler::from_command("  gcc   -m32 -static ").expect("the command names a compiler");
        assert_eq!(compiler.program, "gcc");
        assert_eq!(compiler.leading_arguments, ["-m32", "-static"]);
        assert_eq!(compiler.name(), "gcc -m32 -static");
        assert_eq!(Compiler::from_command(" \t "), None);
    }

    #[test]
    fn the_msvc_version_is_read_from_its_banner_in_any_language() {
        let english = "Microsoft (R) C/C++ Optimizing Compiler Version 19.29.30154 for x64\n\
                       Copyright (C) Microsoft Corporation.  All rights reserved.\n";
        assert_eq!(msvc_version(english), Some((19, 29)));
        let japanese = "Microsoft(R) C/C++ \u{6700}\u{9069}\u{5316}\u{30b3}\u{30f3}\u{30d1}\u{30a4}\u{30e9} \
                        Version 19.16.27051 for x86";
        assert_eq!(msvc_version(japanese), Some((19, 16)));
        assert_eq!(msvc_version("clang-cl: error: no input files"), None);
        assert!((19, 27) < OLDEST_MSVC && (19, 28) >= OLDEST_MSVC);
    }

    #[test]
    fn msvc_options_are_used_for_cl_and_clang_cl_only() {
        assert_eq!(Compiler::named("cl").style(), Style::Msvc);
        assert_eq!(
            Compiler::named("C:/Tools/clang-cl.exe").style(),
            Style::Msvc
        );
        assert_eq!(Compiler::named("clang").style(), Style::Gnu);
        assert_eq!(
            Compiler::named("x86_64-w64-mingw32-gcc").style(),
            Style::Gnu
        );
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
