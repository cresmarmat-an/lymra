/*!
 * The Lymra compiler as a library.
 *
 * The pipeline so far is the front end: `lexer` turns text into tokens,
 * `parser` turns tokens into the syntax tree in `ast`, and `lint` checks rules
 * that need the whole tree. `frontend::analyze` runs all of it for one file.
 */

pub mod ast;
pub mod cli;
pub mod diagnostics;
mod foreign_types;
pub mod frontend;
pub mod lexer;
pub mod lint;
pub mod parser;
pub mod similarity;
pub mod source;
