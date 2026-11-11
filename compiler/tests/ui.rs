/*!
 * Golden tests for what the compiler says.
 *
 * Every `.lym` file under `tests/ui` is checked. A file with errors must
 * have a `.stderr` file next to it that holds the exact report. A file
 * without errors must have a `.tree` file that holds its syntax tree. The
 * wording of an error is part of the language, so a change to it has to show
 * up in a diff.
 */

mod common;

use std::path::Path;

use lymra::ast::print_module;
use lymra::diagnostics::render_report;
use lymra::frontend::analyze;

#[test]
fn every_ui_file_matches_its_golden_output() {
    let compiler_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = common::files_with_extension(&compiler_root.join("tests/ui"), "lym");
    assert!(!files.is_empty(), "no test files found under tests/ui");

    let mut mismatches = Vec::new();
    for path in files {
        // A name relative to the crate, with forward slashes, reads the same on every machine.
        let relative = path
            .strip_prefix(compiler_root)
            .expect("test files are inside the crate");
        let name = relative.to_string_lossy().replace('\\', "/");
        let source = common::source(&name, &common::read(&path));
        let analysis = analyze(&source);

        let (report, tree) = if analysis.diagnostics.is_empty() {
            (None, Some(print_module(&analysis.module)))
        } else {
            (Some(render_report(&analysis.diagnostics, &source)), None)
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
