/*!
 * The programs of the memory model study in `docs/memory` have to stay valid
 * Lymra as the grammar changes, and the study's page has to list each one.
 * They use features of later phases, so they are parsed, not compiled.
 */

mod common;

use lymra::diagnostics::render_report;
use lymra::frontend::analyze;

#[test]
fn every_study_program_parses_and_is_listed() {
    let directory = common::repository_root().join("docs/memory");
    let study = common::read(&directory.join("README.md"));
    let programs = common::files_with_extension(&directory, "lym");
    assert!(
        programs.len() >= 10,
        "expected the study's programs in {}, found {}",
        directory.display(),
        programs.len()
    );

    let mut failures = Vec::new();
    for path in &programs {
        let name = path
            .file_name()
            .expect("a program file has a name")
            .to_string_lossy();
        let source = common::source(&path.display().to_string(), &common::read(path));
        let analysis = analyze(&source);
        if !analysis.diagnostics.is_empty() {
            failures.push(render_report(&analysis.diagnostics, &source));
        }
        if !study.contains(&format!("]({name})")) {
            failures.push(format!("docs/memory/README.md does not link to {name}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
