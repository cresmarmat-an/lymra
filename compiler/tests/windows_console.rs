/*!
 * On Windows, text printed to a console arrives as the characters the
 * program meant, not as UTF-8 bytes read in the console's code page. The
 * test program makes a console of its own, prints a line through the
 * runtime, and reads the characters back from the console's screen.
 */
#![cfg(windows)]

mod common;

use std::process::Command;

use lymra::driver::{TemporaryDirectory, Warnings};

/// Prints a line of non-ASCII text to a new console and exits with 0 if the console shows exactly that text.
const HARNESS: &str = r#"
#include <wchar.h>

int main(void) {
    /* The program leaves any console it shares and makes its own, whose screen it can read back. */
    FreeConsole();
    if (!AllocConsole()) {
        return 2;
    }
    /* A process started with its output redirected keeps it after AllocConsole, so the console becomes standard output here, as it is for a program run in a console window. */
    HANDLE console = CreateFileW(L"CONOUT$", GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL,
                                 OPEN_EXISTING, 0, NULL);
    if (console == INVALID_HANDLE_VALUE || !SetStdHandle(STD_OUTPUT_HANDLE, console)) {
        return 2;
    }
    lymra_start();
    if (!lymra_console[LYMRA_STANDARD_OUTPUT]) {
        return 3;
    }
    static const char line[] = "h\xc3\xa9llo \xe2\x86\x92 w\xc3\xb6rld \xe2\x9c\x93 \xc3\xb1";
    lymra_append(line, sizeof line - 1);
    lymra_end_line();
    lymra_flush();
    static const wchar_t expected[] = L"héllo → wörld ✓ ñ";
    DWORD length = (DWORD)(sizeof expected / sizeof expected[0] - 1);
    wchar_t shown[64];
    DWORD read = 0;
    COORD origin = {0, 0};
    if (!ReadConsoleOutputCharacterW(console, shown, length, origin, &read)) {
        return 4;
    }
    return read == length && wmemcmp(shown, expected, length) == 0 ? 0 : 1;
}
"#;

/// What each exit status of the harness means.
fn explain(code: Option<i32>) -> &'static str {
    match code {
        Some(1) => "the console shows different characters",
        Some(2) => "the program could not make a console",
        Some(3) => "the runtime did not notice that standard output is a console",
        Some(4) => "the console's screen could not be read",
        _ => "the program failed",
    }
}

#[test]
fn a_console_shows_the_characters_that_were_printed() {
    let runtime = common::read(&common::repository_root().join("compiler/src/codegen/runtime.h"));
    let c_source = format!("{runtime}{HARNESS}");
    for compiler in common::c_compilers() {
        let workspace = TemporaryDirectory::new().expect("a temporary directory can be made");
        let executable = workspace.path().join("console.exe");
        compiler
            .compile(&c_source, &executable, None, Warnings::Strict)
            .unwrap_or_else(|error| panic!("{} rejects the harness:\n{error}", compiler.name()));
        let status = Command::new(&executable)
            .status()
            .expect("the harness runs");
        assert!(
            status.success(),
            "{}: {}",
            compiler.name(),
            explain(status.code())
        );
    }
}
