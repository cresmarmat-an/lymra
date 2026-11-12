//! Calls that nest too deeply stop with the stack overflow error under any stack limit, small or unlimited.
#![cfg(unix)]

mod common;

use std::process::Command;

use lymra::codegen;
use lymra::driver::{Compiler, TemporaryDirectory, Warnings};
use lymra::frontend;
use lymra::optimize;

/// Recursion whose base case is too deep to reach on any stack.
const PROGRAM: &str = "\
function depth(integer64 level) returns integer64
    if level == 0
        return 0
    return depth(level - 1) % 1000 + 1

function main()
    print(\"before\")
    print(\"{depth(1_000_000_000)}\")
";

/// Exit status of the shell when it cannot set a stack limit, which the hard limit of a machine can forbid.
const LIMIT_REFUSED: i32 = 99;

#[test]
fn deep_calls_stop_cleanly_under_every_stack_limit() {
    let source = common::source("deep.lym", PROGRAM);
    let mut program = frontend::check_program(&source).expect("the program checks");
    optimize::remove_proven_checks(&mut program);
    let c_source = codegen::generate(&program, &source);
    let compiler = Compiler::find().expect("a C compiler is installed");
    let directory = TemporaryDirectory::new().expect("a temporary directory can be made");
    let executable = directory.path().join("deep");
    compiler
        .compile(&c_source, &executable, program.optimize, Warnings::Strict)
        .expect("the program compiles without warnings");

    let mut tested = 0;
    for limit in ["256", "1024", "8192", "unlimited"] {
        let script = format!("ulimit -s {limit} 2>/dev/null || exit {LIMIT_REFUSED}; exec \"$0\"");
        let output = Command::new("sh")
            .arg("-c")
            .arg(script)
            .arg(&executable)
            .output()
            .expect("sh runs");
        if output.status.code() == Some(LIMIT_REFUSED) {
            continue;
        }
        tested += 1;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(70),
            "stack limit {limit}: {stderr}"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "before\n",
            "stack limit {limit}"
        );
        assert!(
            stderr.starts_with("error: stack overflow: calls nested too deeply\n"),
            "stack limit {limit}: {stderr}"
        );
    }
    assert!(tested > 0, "no stack limit could be set");
}
