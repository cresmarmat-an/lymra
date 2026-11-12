/*!
 * The compiler must survive any input. This test corrupts real source files
 * in thousands of ways and checks that checking and code generation never
 * panic, never hang, and never report a position outside the file.
 *
 * The corruption is pseudo-random with a fixed seed, so a failure can be
 * reproduced by running the test again.
 */

mod common;

use std::path::Path;

use lymra::ast::print_module;
use lymra::codegen;
use lymra::diagnostics::{Diagnostic, render_report};
use lymra::frontend::{self, analyze};
use lymra::optimize;
use lymra::parser::MAX_OPERATIONS;
use lymra::source::Span;

const MUTATIONS_PER_FILE: usize = 400;
const SEED: u64 = 0x4C59_4D52_4131_3031;

/// Text that tends to break parsers: brackets, quotes, layout, and characters wider than one byte.
const FRAGMENTS: [&str; 30] = [
    "(", ")", "[", "]", "{", "}", "\"", "'", "\n", "\r\n", " ", "\t", "    ", "#", "#[", "]#",
    "#!", "=", "==", ".", ",", ":", ";", "\\", "0x", "1e", "é", "\u{a0}", "with", "if",
];

/// A small xorshift generator. The standard library has none, and the test needs repeatable choices.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }

    /// A position in `text` that does not split a character.
    fn boundary(&mut self, text: &str) -> usize {
        let mut index = self.below(text.len() + 1);
        while !text.is_char_boundary(index) {
            index -= 1;
        }
        index
    }
}

fn mutate(text: &str, random: &mut Random) -> String {
    let (first, second) = (random.boundary(text), random.boundary(text));
    let (start, end) = (first.min(second), first.max(second));
    let fragment = FRAGMENTS[random.below(FRAGMENTS.len())];
    match random.below(5) {
        // Insert a fragment.
        0 => format!("{}{fragment}{}", &text[..start], &text[start..]),
        // Delete a short range.
        1 => {
            let mut end = (start + 1 + random.below(8)).min(text.len());
            while !text.is_char_boundary(end) {
                end += 1;
            }
            format!("{}{}", &text[..start], &text[end..])
        }
        // Replace a range with a fragment.
        2 => format!("{}{fragment}{}", &text[..start], &text[end..]),
        // Cut the file short.
        3 => text[..start].to_owned(),
        // Repeat a range.
        _ => format!("{}{}{}", &text[..end], &text[start..end], &text[end..]),
    }
}

fn assert_span_is_valid(span: Span, text: &str, context: &str) {
    let in_bounds = span.start <= span.end && span.end as usize <= text.len();
    assert!(in_bounds, "span {span:?} is outside the source\n{context}");
    let on_boundaries =
        text.is_char_boundary(span.start as usize) && text.is_char_boundary(span.end as usize);
    assert!(on_boundaries, "span {span:?} splits a character\n{context}");
}

fn assert_diagnostics_are_valid(diagnostics: &[Diagnostic], text: &str, context: &str) {
    for diagnostic in diagnostics {
        assert_span_is_valid(diagnostic.primary.span, text, context);
        for label in &diagnostic.secondary {
            assert_span_is_valid(label.span, text, context);
        }
        if let Some(suggestion) = &diagnostic.help.suggestion {
            assert_span_is_valid(suggestion.span, text, context);
        }
        assert!(
            !diagnostic.message.is_empty() && !diagnostic.help.message.is_empty(),
            "{context}"
        );
    }
}

/// Runs `test` on a thread with the stack the compiler's stages are documented to need.
fn on_compiler_stack(test: impl FnOnce() + Send) {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(lymra::STACK_SIZE)
            .spawn_scoped(scope, test)
            .expect("the system starts a thread with the compiler's stack")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    });
}

fn check_survives(name: &str, text: &str) {
    let source = common::source(name, text);
    let context = format!("--- source ({name}):\n{text}\n---");

    let analysis = analyze(&source);
    assert_diagnostics_are_valid(&analysis.diagnostics, text, &context);
    // Rendering and printing walk every span and every node, so they must not panic either.
    let report = render_report(&analysis.diagnostics, &source);
    assert_eq!(
        report.is_empty(),
        analysis.diagnostics.is_empty(),
        "{context}"
    );
    assert!(
        print_module(&analysis.module).starts_with("module\n"),
        "{context}"
    );

    // A file with valid syntax goes on to the semantic checker and, if that passes, to the optimizer and code generation.
    match frontend::check(&source) {
        Ok(mut program) => {
            optimize::remove_proven_checks(&mut program);
            let c_source = codegen::generate(&program, &source);
            assert!(
                c_source.starts_with("/* Generated by the Lymra compiler"),
                "{context}"
            );
        }
        Err(diagnostics) => {
            assert!(
                !diagnostics.is_empty(),
                "a rejected file has to say why\n{context}"
            );
            assert_diagnostics_are_valid(&diagnostics, text, &context);
            render_report(&diagnostics, &source);
        }
    }
}

#[test]
fn corrupted_sources_never_crash_the_compiler() {
    on_compiler_stack(check_corrupted_sources);
}

fn check_corrupted_sources() {
    let tests_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let files = common::files_with_extension(&tests_directory, "lym");
    assert!(!files.is_empty());

    let mut random = Random(SEED);
    for path in files {
        let original = common::read(&path);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut text = original.clone();
        for round in 0..MUTATIONS_PER_FILE {
            // Corruption accumulates for a while, then starts again from the original file.
            if round % 8 == 0 {
                text.clone_from(&original);
            }
            text = mutate(&text, &mut random);
            check_survives(&name, &text);
        }
    }
}

#[test]
fn pathological_inputs_are_handled() {
    on_compiler_stack(check_pathological_inputs);
}

fn check_pathological_inputs() {
    let longest_expression = usize::try_from(MAX_OPERATIONS).expect("the limit fits a usize") + 1;
    let cases = [
        String::new(),
        "\n".repeat(10_000),
        "(".repeat(50_000),
        ")".repeat(50_000),
        "[".repeat(50_000),
        "\"{".repeat(50_000),
        "#[".repeat(50_000),
        "- ".repeat(50_000),
        "not ".repeat(50_000),
        "x.".repeat(50_000),
        "f(".repeat(50_000),
        "a + ".repeat(50_000),
        "packed ".repeat(50_000),
        "function f()\n    if x\n".to_owned(),
        (0..200)
            .map(|depth| format!("{}if x\n", "    ".repeat(depth)))
            .collect(),
        (0..200)
            .map(|depth| format!("{}x\n", " ".repeat(depth)))
            .collect(),
        "\t \t \t x\n".repeat(100),
        "\u{feff}\u{feff}\u{200b}x = \u{a0}1\r\r\n".repeat(50),
        "\"\\".to_owned(),
        "'".to_owned(),
        "#!".to_owned(),
        "#!\u{e9}\u{e9} \u{e9}".to_owned(),
        "0x".repeat(1000),
        "1.".repeat(1000),
        "1e+".repeat(1000),
        // The longest expressions the parser accepts, through every stage that follows them recursively.
        format!(
            "function total(integer64 value) returns integer64\n    return {}\n\nfunction main()\n    print(\"{{total(1)}}\")\n",
            vec!["value"; longest_expression].join(" + ")
        ),
        format!(
            "function main()\n    constant total = {}\n    print(\"{{total}}\")\n",
            vec!["1"; longest_expression].join(" - ")
        ),
        format!(
            "function main()\n    print(\"{{{}}}\")\n",
            vec!["true"; longest_expression].join(" and ")
        ),
    ];
    for (index, case) in cases.iter().enumerate() {
        check_survives(&format!("pathological case {index}"), case);
    }
}
