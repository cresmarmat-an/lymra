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

use crate::ir::{Program, Value};

/// Removes every check the analysis proves can never fail.
pub fn remove_proven_checks(program: &mut Program<'_>) {
    let constants: Vec<Value> = program
        .constants
        .iter()
        .map(|constant| constant.value.clone())
        .collect();
    for function in &mut program.functions {
        analysis::optimize_function(function, &constants);
    }
}
