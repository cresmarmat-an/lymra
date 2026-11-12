/*!
 * The optimizer: removes runtime checks that can never fail.
 *
 * Lymra checks every integer operation for overflow and division by zero, in
 * every build mode (spec section 4.2). A check also keeps the C compiler from
 * treating the arithmetic freely: it cannot turn a loop into a formula, or
 * vectorize it, when an overflow has to stop the program at exactly the
 * operation where it happens. So every check the compiler can prove
 * unnecessary is worth removing.
 *
 * The proofs come from interval analysis (see `analysis.rs`). An operation
 * whose operands lie within proven bounds, and whose every possible result
 * fits its type, cannot fail, and is compiled to plain C arithmetic. When the
 * analysis cannot prove that, the check stays. It never weakens a guarantee:
 * a program behaves the same with or without this pass, only faster.
 */

mod analysis;
mod interval;
mod loops;
mod proof;

pub use proof::UnprovenIndex;

use crate::diagnostics::Diagnostic;
use crate::ir::{Program, Value};
use crate::source::SourceFile;

/// Removes every check the analysis proves can never fail, and returns the index checks that stay.
pub fn remove_proven_checks(program: &mut Program<'_>) -> Vec<UnprovenIndex> {
    let constants: Vec<Value> = program
        .constants
        .iter()
        .map(|constant| constant.value.clone())
        .collect();
    program
        .functions
        .iter_mut()
        .flat_map(|function| analysis::optimize_function(function, &constants))
        .collect()
}

/**
 * The errors `#!prove bounds` asks for: one for each index of `program` the
 * analysis cannot prove inside its array. The program itself keeps its
 * checks, so a build can still compare it with the optimized one.
 */
pub fn unproven_bounds(program: &Program<'_>, source: &SourceFile) -> Vec<Diagnostic> {
    let mut proof = program.clone();
    remove_proven_checks(&mut proof)
        .iter()
        .map(|unproven| unproven.diagnostic(source))
        .collect()
}
