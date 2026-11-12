//! The front end: everything that happens to one file before code generation.

use crate::ast::Module;
use crate::diagnostics::Diagnostic;
use crate::ir::Program;
use crate::source::SourceFile;
use crate::{lexer, lint, optimize, parser, semantic};

/// The syntax tree of a file and every syntax error found while building it.
pub struct Analysis<'src> {
    pub module: Module<'src>,
    /// Errors in source order. The file is valid Lymra syntax when this is empty.
    pub diagnostics: Vec<Diagnostic>,
}

/// Lexes, parses and lints a file. It never stops at the first error.
pub fn analyze(source: &SourceFile) -> Analysis<'_> {
    let lexed = lexer::lex(source);
    let parsed = parser::parse(source, &lexed.tokens);

    let mut diagnostics = lexed.diagnostics;
    diagnostics.extend(parsed.diagnostics);
    diagnostics.extend(lint::naming::check(&parsed.module));
    // A stable sort keeps the lexer's report first when two errors start at the same place.
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.span.start);
    Analysis {
        module: parsed.module,
        diagnostics,
    }
}

/**
 * Checks a file completely: syntax first, then names, types and control flow.
 * Meaning is only checked once the syntax is valid, because a broken tree
 * would produce errors that are only echoes of the syntax error.
 */
pub fn check(source: &SourceFile) -> Result<Program<'_>, Vec<Diagnostic>> {
    let analysis = analyze(source);
    if !analysis.diagnostics.is_empty() {
        return Err(analysis.diagnostics);
    }
    let program = semantic::check(source, &analysis.module)?;
    // `#!prove bounds` turns every index the optimizer cannot prove into an error.
    if program.prove_bounds {
        let unproven = optimize::unproven_bounds(&program, source);
        if !unproven.is_empty() {
            return Err(unproven);
        }
    }
    Ok(program)
}

/// Checks a file that is meant to run, which also needs a `main` function.
pub fn check_program(source: &SourceFile) -> Result<Program<'_>, Vec<Diagnostic>> {
    let program = check(source)?;
    if program.main.is_none() {
        return Err(vec![semantic::missing_main_error()]);
    }
    Ok(program)
}
