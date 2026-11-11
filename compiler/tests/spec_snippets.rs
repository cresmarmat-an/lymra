/*!
 * The spec and the parser must agree: every `lymra` code block in
 * `docs/spec.md` has to parse without errors. This is the exit criterion of
 * roadmap phase 0.
 */

mod common;

use lymra::diagnostics::render_report;
use lymra::frontend::analyze;

const FENCE_OPEN: &str = "```lymra";
const FENCE_CLOSE: &str = "```";

/// A code block from the spec, with the line it starts on for error messages.
struct Snippet {
    line: usize,
    code: String,
}

fn snippets(markdown: &str) -> Vec<Snippet> {
    let mut found = Vec::new();
    let mut current: Option<Snippet> = None;
    for (index, line) in markdown.lines().enumerate() {
        match &mut current {
            None if line.trim() == FENCE_OPEN => {
                current = Some(Snippet {
                    line: index + 2,
                    code: String::new(),
                })
            }
            None => {}
            Some(_) if line.trim() == FENCE_CLOSE => found.extend(current.take()),
            Some(snippet) => {
                snippet.code.push_str(line);
                snippet.code.push('\n');
            }
        }
    }
    found
}

/// The errors a snippet produces, rendered, or `None` if it parses.
fn errors_in(name: &str, code: &str) -> Option<String> {
    let source = common::source(name, code);
    let analysis = analyze(&source);
    (!analysis.diagnostics.is_empty()).then(|| render_report(&analysis.diagnostics, &source))
}

/// Statements are only valid inside a function, so a snippet of statements is tried as a function body.
fn as_function_body(code: &str) -> String {
    let body: String = code
        .lines()
        .map(|line| {
            if line.is_empty() {
                "\n".to_owned()
            } else {
                format!("    {line}\n")
            }
        })
        .collect();
    format!("function snippet()\n{body}")
}

#[test]
fn every_lymra_snippet_in_the_spec_parses() {
    let spec = common::read(&common::repository_root().join("docs/spec.md"));
    let snippets = snippets(&spec);
    assert!(
        snippets.len() >= 10,
        "expected the spec to contain code snippets, found {}",
        snippets.len()
    );

    let mut failures = Vec::new();
    for snippet in &snippets {
        let name = format!("docs/spec.md:{}", snippet.line);
        let Some(as_module) = errors_in(&name, &snippet.code) else {
            continue;
        };
        let Some(as_body) = errors_in(&name, &as_function_body(&snippet.code)) else {
            continue;
        };
        failures.push(format!(
            "the snippet at {name} does not parse.\n--- as declarations:\n{as_module}\n--- as statements:\n{as_body}"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
