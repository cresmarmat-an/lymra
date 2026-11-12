//! Helpers shared by the integration tests.

// Each test file is compiled as its own crate and uses only part of this module.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use lymra::driver::Compiler;
use lymra::source::SourceFile;

/// When set, golden files are rewritten to match the compiler instead of being compared with it.
const BLESS_VARIABLE: &str = "LYMRA_BLESS";

pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub fn should_bless() -> bool {
    std::env::var_os(BLESS_VARIABLE).is_some()
}

pub fn source(name: &str, text: &str) -> SourceFile {
    SourceFile::new(name, text).expect("test sources are small")
}

pub fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/**
 * Compares `actual` with the golden file at `path`, where `None` means the
 * file must not exist. In bless mode the file is updated instead. Returns a
 * description of the mismatch, if any.
 */
pub fn check_golden(path: &Path, actual: Option<&str>) -> Option<String> {
    let expected = fs::read_to_string(path).ok();
    if expected.as_deref() == actual {
        return None;
    }
    if should_bless() {
        match actual {
            Some(text) => fs::write(path, text)
                .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display())),
            None => fs::remove_file(path)
                .unwrap_or_else(|error| panic!("cannot remove {}: {error}", path.display())),
        }
        return None;
    }
    let show = |text: Option<&str>| text.map_or("(no file)".to_owned(), |text| format!("\n{text}"));
    Some(format!(
        "{} is out of date\n--- expected: {}\n--- actual: {}",
        path.display(),
        show(expected.as_deref()),
        show(actual)
    ))
}

/// Fails the test with every mismatch found, and says how to accept the new output.
pub fn assert_no_mismatches(mismatches: &[String]) {
    assert!(
        mismatches.is_empty(),
        "{}\n\n{} golden file(s) differ. If the new output is right, run the tests with {BLESS_VARIABLE}=1 to accept it.",
        mismatches.join("\n\n"),
        mismatches.len()
    );
}

/// Every file under `directory` with the given extension, in a stable order.
pub fn files_with_extension(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = fs::read_dir(&current)
            .unwrap_or_else(|error| panic!("cannot list {}: {error}", current.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|error| panic!("cannot list {}: {error}", current.display()))
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|found| found == extension) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/**
 * The C compilers the tests build with: GCC, Clang and MSVC's `cl` where they
 * are installed, and the one in `CC` when it is set, with its arguments, as
 * in `CC="gcc -m32"`. Programs have to behave the same with every one.
 */
pub fn c_compilers() -> Vec<Compiler> {
    let mut compilers: Vec<Compiler> = ["gcc", "clang", "cl"]
        .iter()
        .map(|name| Compiler::named(name))
        .filter(Compiler::is_available)
        .collect();
    if let Some(chosen) = Compiler::from_environment() {
        if chosen.is_available() && !compilers.contains(&chosen) {
            compilers.push(chosen);
        }
    }
    assert!(
        !compilers.is_empty(),
        "these tests need a C compiler; install GCC, Clang or MSVC"
    );
    compilers
}
