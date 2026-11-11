//! The front end: everything that happens to one file before type checking.

use crate::ast::Module;
use crate::diagnostics::Diagnostic;
use crate::source::SourceFile;
use crate::{lexer, lint, parser};

/// The syntax tree of a file and every error found while building it.
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
