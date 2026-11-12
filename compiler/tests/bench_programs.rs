//! The benchmark programs in `bench/programs` have to stay valid Lymra as the language changes.

mod common;

use lymra::diagnostics::render_report;
use lymra::frontend;

#[test]
fn every_benchmark_program_passes_the_checker() {
    let programs = common::repository_root().join("bench/programs");
    let mut checked = 0;
    for entry in std::fs::read_dir(&programs).expect("bench/programs is readable") {
        let path = entry
            .expect("bench/programs is readable")
            .path()
            .join("program.lym");
        let text = common::read(&path);
        let source = common::source(&path.display().to_string(), &text);
        if let Err(diagnostics) = frontend::check_program(&source) {
            panic!(
                "{} does not pass the checker:\n{}",
                path.display(),
                render_report(&diagnostics, &source)
            );
        }
        checked += 1;
    }
    assert!(
        checked > 0,
        "no benchmark programs in {}",
        programs.display()
    );
}
