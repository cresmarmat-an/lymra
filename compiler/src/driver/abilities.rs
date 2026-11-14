//! What a GCC-style C compiler can do, found out once by asking it.

use std::fs;
use std::process::{Command, Stdio};

use super::{Compiler, TemporaryDirectory};

/// The stack every Lymra program gets: the default of Linux and macOS, given on every system.
pub(super) const PROGRAM_STACK_SIZE: u32 = 8 << 20;

/// What one compiler answered, which every build with it uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Abilities {
    /**
     * Whether it takes `-fstack-clash-protection` without a warning. A
     * function whose frame is larger than a page then touches its stack a
     * page at a time, so it cannot step over the guard page that stops an
     * overflow.
     */
    pub probes_stack: bool,
    /// The option that gives a program `PROGRAM_STACK_SIZE` of stack, where the system's default is smaller.
    pub stack_option: Option<String>,
    /**
     * The options that make every float operation round to its own type, so
     * a program computes the same floats on every system: no fusing of a
     * multiply and an add, which processors with fused multiply-add would
     * round once instead of twice, and on 32-bit x86, SSE2 instead of the x87
     * unit, which keeps floats wider than their type.
     */
    pub float_options: Vec<&'static str>,
}

/// Turns off fusing, which Clang does by default where the processor can.
const NO_CONTRACTION: [&str; 1] = ["-ffp-contract=off"];

/// Selects SSE2 for floats. Only compilers for x86 take these.
const SSE_MATH: [&str; 2] = ["-msse2", "-mfpmath=sse"];

impl Abilities {
    pub fn of(compiler: &Compiler) -> Self {
        let float_options = [&NO_CONTRACTION[..], &SSE_MATH[..]]
            .into_iter()
            .filter(|options| accepts(compiler, options))
            .flatten()
            .copied()
            .collect();
        Self {
            probes_stack: accepts(compiler, &["-fstack-clash-protection"]),
            stack_option: target(compiler).and_then(|target| stack_option(&target)),
            float_options,
        }
    }
}

/// Whether `compiler` builds a clean file with `options` added and every warning an error.
fn accepts(compiler: &Compiler, options: &[&str]) -> bool {
    let Ok(workspace) = TemporaryDirectory::new() else {
        return false;
    };
    let source = workspace.path().join("probe.c");
    if fs::write(&source, "int main(void) {\n    return 0;\n}\n").is_err() {
        return false;
    }
    Command::new(&compiler.program)
        .args(&compiler.leading_arguments)
        .args(["-std=c11", "-Werror"])
        .args(options)
        .args(["-c", "-o"])
        .arg(workspace.path().join("probe.o"))
        .arg(&source)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The system the compiler builds for, as in `x86_64-w64-mingw32`.
fn target(compiler: &Compiler) -> Option<String> {
    let output = Command::new(&compiler.program)
        .args(&compiler.leading_arguments)
        .arg("-dumpmachine")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/**
 * The linker option for a `PROGRAM_STACK_SIZE` stack on `target`. Windows
 * gives a program 1 MiB unless its executable asks for more; the other
 * systems give 8 MiB already.
 */
fn stack_option(target: &str) -> Option<String> {
    if !target.contains("windows") && !target.contains("mingw") && !target.contains("cygwin") {
        return None;
    }
    // MinGW links with the GNU linker; the MSVC target links with Microsoft's.
    let option = if target.contains("msvc") {
        format!("-Wl,/STACK:{PROGRAM_STACK_SIZE}")
    } else {
        format!("-Wl,--stack,{PROGRAM_STACK_SIZE}")
    };
    Some(option)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_windows_targets_ask_for_a_larger_stack() {
        assert_eq!(stack_option("x86_64-linux-gnu"), None);
        assert_eq!(stack_option("aarch64-apple-darwin23.4.0"), None);
        assert_eq!(
            stack_option("x86_64-w64-mingw32").as_deref(),
            Some("-Wl,--stack,8388608")
        );
        assert_eq!(
            stack_option("x86_64-pc-windows-msvc").as_deref(),
            Some("-Wl,/STACK:8388608")
        );
        assert_eq!(
            stack_option("x86_64-pc-cygwin").as_deref(),
            Some("-Wl,--stack,8388608")
        );
    }
}
