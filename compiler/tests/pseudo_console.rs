/*!
 * Windows Terminal, VS Code and other terminals run programs in a pseudo
 * console, which hands the terminal UTF-8 text. What a Lymra program prints,
 * and the runtime error that stops it, have to reach the terminal as the
 * characters the program meant. The test runs a program in a pseudo console
 * and reads what a terminal would receive.
 */
#![cfg(windows)]

mod common;

use std::process::Command;

use lymra::codegen;
use lymra::driver::{TemporaryDirectory, Warnings};
use lymra::frontend;

/// Text beyond ASCII: accented letters, an arrow and a check mark.
const LINE: &str = "h\u{e9}llo \u{2192} w\u{f6}rld \u{2713} \u{f1}";

/// The status a Lymra program ends with after a runtime error.
const RUNTIME_ERROR_STATUS: i32 = 70;

/// Runs the program its argument names in a pseudo console, passes on what the console sends, and exits with the program's status.
const HOST: &str = r#"
#define _WIN32_WINNT 0x0A00
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

/*
 * The pseudo console functions are looked up in kernel32 when the host runs,
 * so it builds with any SDK, including ones that do not declare them yet.
 */
typedef void *pseudo_console;
typedef HRESULT(WINAPI *create_function)(COORD size, HANDLE input, HANDLE output, DWORD flags,
                                         pseudo_console *console);
typedef void(WINAPI *close_function)(pseudo_console console);
/* PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, which older headers lack. */
#define PSEUDO_CONSOLE_ATTRIBUTE ((DWORD_PTR)0x00020016)

static HANDLE console_output;

/* Passes on everything the pseudo console sends, byte for byte, until it closes, as a terminal would read it. */
static DWORD WINAPI copy_console_output(LPVOID unused) {
    (void)unused;
    HANDLE standard_output = GetStdHandle(STD_OUTPUT_HANDLE);
    char buffer[4096];
    DWORD count = 0;
    while (ReadFile(console_output, buffer, sizeof buffer, &count, NULL) && count > 0) {
        DWORD written = 0;
        WriteFile(standard_output, buffer, count, &written, NULL);
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2) {
        return 2;
    }
    HMODULE kernel = GetModuleHandleA("kernel32.dll");
    /* A cast through void (*)(void) is how C converts between unrelated function pointer types. */
    create_function create_console =
        (create_function)(void (*)(void))GetProcAddress(kernel, "CreatePseudoConsole");
    close_function close_console = (close_function)(void (*)(void))GetProcAddress(kernel, "ClosePseudoConsole");
    if (create_console == NULL || close_console == NULL) {
        return 3;
    }
    HANDLE input_read = NULL;
    HANDLE input_write = NULL;
    HANDLE output_write = NULL;
    if (!CreatePipe(&input_read, &input_write, NULL, 0) || !CreatePipe(&console_output, &output_write, NULL, 0)) {
        return 4;
    }
    /* Wide enough that no line of the program's output wraps. */
    COORD size = {300, 50};
    pseudo_console console = NULL;
    if (FAILED(create_console(size, input_read, output_write, 0, &console))) {
        return 5;
    }
    /* The pseudo console holds its own copies of these ends. */
    CloseHandle(input_read);
    CloseHandle(output_write);
    HANDLE reader = CreateThread(NULL, 0, copy_console_output, NULL, 0, NULL);
    if (reader == NULL) {
        return 6;
    }

    SIZE_T list_size = 0;
    InitializeProcThreadAttributeList(NULL, 1, 0, &list_size);
    LPPROC_THREAD_ATTRIBUTE_LIST attributes = (LPPROC_THREAD_ATTRIBUTE_LIST)HeapAlloc(GetProcessHeap(), 0, list_size);
    if (attributes == NULL || !InitializeProcThreadAttributeList(attributes, 1, 0, &list_size) ||
        !UpdateProcThreadAttribute(attributes, 0, PSEUDO_CONSOLE_ATTRIBUTE, console, sizeof console, NULL, NULL)) {
        return 7;
    }
    STARTUPINFOEXA startup;
    memset(&startup, 0, sizeof startup);
    startup.StartupInfo.cb = sizeof startup;
    /*
     * Null standard handles, which Windows replaces with the pseudo console's
     * when the program starts. Without them, Windows hands the program this
     * host's own standard handles, pipes here, and its output would go around
     * the pseudo console, as it does not in a terminal.
     */
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.lpAttributeList = attributes;
    char command[MAX_PATH + 3];
    snprintf(command, sizeof command, "\"%s\"", argv[1]);
    PROCESS_INFORMATION process;
    if (!CreateProcessA(NULL, command, NULL, NULL, FALSE, EXTENDED_STARTUPINFO_PRESENT, NULL, NULL,
                        &startup.StartupInfo, &process)) {
        return 8;
    }
    DWORD status = 9;
    if (WaitForSingleObject(process.hProcess, 60000) != WAIT_OBJECT_0 || !GetExitCodeProcess(process.hProcess, &status)) {
        status = 9;
    }
    /* Closing the console sends what it still holds, then ends its output, which ends the copy. */
    close_console(console);
    WaitForSingleObject(reader, 30000);
    CloseHandle(input_write);
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    DeleteProcThreadAttributeList(attributes);
    HeapFree(GetProcessHeap(), 0, attributes);
    return (int)status;
}
"#;

/// What an exit status of the host means when the program did not get to run.
fn explain(code: Option<i32>) -> &'static str {
    match code {
        Some(3) => "this Windows has no pseudo console",
        Some(4..=7) => "the pseudo console could not be set up",
        Some(8) => "the program could not be started in the pseudo console",
        Some(9) => "the program did not finish",
        _ => "the program ended with the wrong status",
    }
}

#[test]
fn a_pseudo_console_receives_the_text_and_the_error_a_program_writes() {
    let text = format!(
        "function main()\n    constant integer64 count = 3\n    print(\"{LINE}\")\n    assert count > 5\n"
    );
    let source = common::source("console.lym", &text);
    let program = frontend::check_program(&source).expect("the program checks");
    let c_source = codegen::generate(&program, &source);
    for compiler in common::c_compilers() {
        let workspace = TemporaryDirectory::new().expect("a temporary directory can be made");
        let executable = workspace.path().join("program.exe");
        let host = workspace.path().join("host.exe");
        compiler
            .compile(&c_source, &executable, program.optimize, Warnings::Strict)
            .unwrap_or_else(|error| panic!("{} rejects the program:\n{error}", compiler.name()));
        compiler
            .compile(HOST, &host, None, Warnings::Strict)
            .unwrap_or_else(|error| panic!("{} rejects the host:\n{error}", compiler.name()));
        let output = Command::new(&host)
            .arg(&executable)
            .output()
            .expect("the host runs");
        let received = String::from_utf8_lossy(&output.stdout);
        // The host writes nothing of its own there, so anything on it went around the pseudo console.
        assert!(
            output.stderr.is_empty(),
            "{}: the program wrote around the pseudo console:\n{:?}",
            compiler.name(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.status.code(),
            Some(RUNTIME_ERROR_STATUS),
            "{}: {}; the terminal received:\n{received:?}",
            compiler.name(),
            explain(output.status.code())
        );
        assert!(
            received.contains(LINE),
            "{}: the terminal did not receive {LINE:?}:\n{received:?}",
            compiler.name()
        );
        assert!(
            received.contains("error: assertion failed"),
            "{}: the terminal did not receive the runtime error:\n{received:?}",
            compiler.name()
        );
    }
}
