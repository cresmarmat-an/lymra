/*!
 * Golden tests for what the compiler says.
 *
 * Every `.lym` file under `tests/ui` is checked for syntax, and every one
 * under `tests/check` for syntax and meaning. A file with errors must have a
 * `.stderr` file next to it that holds the exact report. A valid file under
 * `tests/ui` must have a `.tree` file that holds its syntax tree. The wording
 * of an error is part of the language, so a change to it has to show up in a
 * diff.
 */

mod common;

use std::path::Path;

use lymra::ast::print_module;
use lymra::diagnostics::render_report;
use lymra::frontend::{self, analyze};

/// How much of the front end a directory of golden tests runs.
#[derive(Clone, Copy)]
enum Stage {
    /// Syntax only: lexing, parsing and the naming check. A valid file records its syntax tree.
    Syntax,
    /// Syntax, then names, types and control flow. Every file is expected to have errors.
    Meaning,
}

fn check_directory(directory: &str, stage: Stage) {
    let compiler_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = common::files_with_extension(&compiler_root.join(directory), "lym");
    assert!(!files.is_empty(), "no test files found under {directory}");

    let mut mismatches = Vec::new();
    for path in files {
        // A name relative to the crate, with forward slashes, reads the same on every machine.
        let relative = path
            .strip_prefix(compiler_root)
            .expect("test files are inside the crate");
        let name = relative.to_string_lossy().replace('\\', "/");
        let source = common::source(&name, &common::read(&path));

        let (report, tree) = match stage {
            Stage::Syntax => {
                let analysis = analyze(&source);
                if analysis.diagnostics.is_empty() {
                    (None, Some(print_module(&analysis.module)))
                } else {
                    (Some(render_report(&analysis.diagnostics, &source)), None)
                }
            }
            Stage::Meaning => match frontend::check(&source) {
                Ok(_) => (None, None),
                Err(diagnostics) => (Some(render_report(&diagnostics, &source)), None),
            },
        };
        mismatches.extend(common::check_golden(
            &path.with_extension("stderr"),
            report.as_deref(),
        ));
        mismatches.extend(common::check_golden(
            &path.with_extension("tree"),
            tree.as_deref(),
        ));
    }
    common::assert_no_mismatches(&mismatches);
}

#[test]
fn every_syntax_test_matches_its_golden_output() {
    check_directory("tests/ui", Stage::Syntax);
}

#[test]
fn every_meaning_test_matches_its_golden_output() {
    check_directory("tests/check", Stage::Meaning);
}
