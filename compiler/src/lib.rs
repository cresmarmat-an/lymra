/*!
 * The Lymra compiler as a library.
 *
 * The pipeline: `lexer` turns text into tokens, `parser` turns tokens into the
 * syntax tree in `ast`, and `lint` checks style rules on the tree. Then
 * `semantic` resolves names and checks types, producing the checked program in
 * `ir`. `codegen` turns that into C, and `driver` compiles the C with the
 * system's C compiler. `frontend` runs the checking stages for one file.
 */

pub mod ast;
pub mod cli;
pub mod codegen;
pub mod diagnostics;
pub mod driver;
mod foreign_types;
pub mod frontend;
pub mod ir;
pub mod lexer;
pub mod lint;
pub mod parser;
pub mod semantic;
pub mod similarity;
pub mod source;
