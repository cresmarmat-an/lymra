//! The `lymra` executable. All behavior lives in the library; this only connects it to the process.

use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    // Arguments that are not valid Unicode are kept readable instead of aborting the program.
    let arguments: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    lymra::cli::run(
        &arguments,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
    .into()
}
