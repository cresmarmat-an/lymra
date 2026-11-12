/*!
 * Keeps `docs/errors/` honest. The example on each page has to produce the
 * error the page is about, every fix has to be accepted, and the index has to
 * list every code the compiler can report.
 */

mod common;

use lymra::diagnostics::{ErrorCode, render_report};
use lymra::frontend::{self, analyze};

const FENCE_OPEN: &str = "```lymra";
const FENCE_CLOSE: &str = "```";

/// The `lymra` code blocks in one section of a page, which runs from `heading` to the next heading.
fn code_blocks(page: &str, heading: &str) -> Vec<String> {
    let section = page.split_once(heading).map_or("", |(_, after)| after);
    let section = section
        .split_once("\n## ")
        .map_or(section, |(before, _)| before);

    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in section.lines() {
        match &mut current {
            None if line.trim() == FENCE_OPEN => current = Some(String::new()),
            None => {}
            Some(_) if line.trim() == FENCE_CLOSE => blocks.extend(current.take()),
            Some(block) => {
                block.push_str(line);
                block.push('\n');
            }
        }
    }
    blocks
}

/// Every error code the compiler reports for a source, from syntax to meaning.
fn reported_codes(text: &str) -> Vec<ErrorCode> {
    let source = common::source("example.lym", text);
    match frontend::check(&source) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect(),
    }
}

#[test]
fn each_example_produces_the_error_its_page_explains() {
    for &code in ErrorCode::ALL {
        // A file that is not UTF-8 cannot be written as an example inside a UTF-8 page.
        if code == ErrorCode::L0115 {
            continue;
        }
        let examples = code_blocks(code.explanation(), "## Example");
        assert!(!examples.is_empty(), "docs/errors/{code}.md has no example");
        for example in examples {
            let reported = reported_codes(&example);
            assert!(
                reported.contains(&code),
                "the example in docs/errors/{code}.md reports {reported:?}, not {code}:\n{example}"
            );
        }
    }
}

/// True for the codes the semantic checker reports. Their pages show complete programs.
fn is_about_meaning(code: ErrorCode) -> bool {
    const BINDING_RULES: [ErrorCode; 7] = [
        ErrorCode::L0201,
        ErrorCode::L0202,
        ErrorCode::L0206,
        ErrorCode::L0207,
        ErrorCode::L0208,
        ErrorCode::L0209,
        ErrorCode::L0210,
    ];
    code.as_str() >= "L0400" || code == ErrorCode::L0309 || BINDING_RULES.contains(&code)
}

/**
 * A fix has to be valid syntax and must not produce the error its page is
 * about. On pages about meaning, the fix is a complete program that passes
 * every check. Fixes on syntax pages may use names they do not declare.
 */
#[test]
fn each_fix_removes_its_error() {
    for &code in ErrorCode::ALL {
        for fix in code_blocks(code.explanation(), "## Fix") {
            let source = common::source("fix.lym", &fix);
            let syntax = analyze(&source).diagnostics;
            assert!(
                syntax.is_empty(),
                "the fix in docs/errors/{code}.md has syntax errors:\n{}",
                render_report(&syntax, &source)
            );
            let reported = reported_codes(&fix);
            assert!(
                !reported.contains(&code),
                "the fix in docs/errors/{code}.md still reports {code}:\n{fix}"
            );
            if is_about_meaning(code) {
                assert!(
                    reported.is_empty(),
                    "the fix in docs/errors/{code}.md is not a complete program; it reports {reported:?}:\n{fix}"
                );
            }
        }
    }
}

#[test]
fn the_index_lists_every_code() {
    let mut index = String::from(
        "# Lymra error codes\n\n\
         Every error the compiler reports has a code. Each page says what the rule is, why it exists, and how to fix the code.\n\
         `lymra explain <code>` prints the same page.\n\n\
         The first two digits group the codes: 01 is source text and layout, 02 is bindings, 03 is grammar.\n\
         From 04 on, they are the chapter of the [specification](../spec.md) the rule comes from.\n\n\
         | Code | Rule |\n|---|---|\n",
    );
    for code in ErrorCode::ALL {
        index.push_str(&format!("| [{code}]({code}.md) | {} |\n", code.summary()));
    }
    let path = common::repository_root().join("docs/errors/README.md");
    common::assert_no_mismatches(
        &common::check_golden(&path, Some(&index))
            .into_iter()
            .collect::<Vec<_>>(),
    );
}
