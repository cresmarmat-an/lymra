/*!
 * The Lymra compiler as a library.
 *
 * The pipeline: `lexer` turns text into tokens, `parser` turns tokens into the
 * syntax tree in `ast`, and `lint` checks style rules on the tree. Then
 * `semantic` resolves names and checks types, producing the checked program in
 * `ir`. `optimize` removes the runtime checks it proves can never fail,
 * `codegen` turns the program into C, and `driver` compiles the C with the
 * system's C compiler. `frontend` runs the checking stages for one file.
 */

/**
 * The stack the compiler's stages need, with room to spare, for the deepest
 * expression the parser accepts, even in a build without optimizations. The
 * stages follow expressions recursively, and a thread's default stack can be
 * as small as 1 MiB, so a program that runs them gives them a thread with this
 * much. A stack only reserves address space until it is used.
 */
pub const STACK_SIZE: usize = 64 << 20;

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
pub mod optimize;
pub mod parser;
pub mod semantic;
pub mod similarity;
pub mod source;
