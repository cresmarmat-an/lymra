//! The `lymra` executable. All behavior lives in the library; this only connects it to the process.

use std::ffi::OsString;
use std::io;
use std::process::ExitCode;
use std::thread;

/// The status Rust gives a program whose main thread panics.
const PANIC_STATUS: u8 = 101;

fn main() -> ExitCode {
    match thread::Builder::new()
        .name("lymra".to_owned())
        .stack_size(lymra::STACK_SIZE)
        .spawn(compile)
    {
        // The panic message is already written by then, as it would be from the main thread.
        Ok(compiler) => compiler
            .join()
            .unwrap_or_else(|_| ExitCode::from(PANIC_STATUS)),
        // Without a thread of its own, the compiler still runs, on the stack this thread has.
        Err(_) => compile(),
    }
}

fn compile() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    lymra::cli::run(
        &arguments,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
    .into()
}
