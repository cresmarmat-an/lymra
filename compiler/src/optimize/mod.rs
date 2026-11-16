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
 * The proofs come from interval analysis (see `analysis.rs`), with what is
 * known across calls (see `facts.rs`). An operation whose operands lie within
 * proven bounds, and whose every possible result fits its type, cannot fail,
 * and is compiled to plain C arithmetic. When the analysis cannot prove that,
 * the check stays. It never weakens a guarantee: a program behaves the same
 * with or without this pass, only faster.
 */

mod analysis;
mod facts;
mod interval;
mod loops;
mod proof;
mod while_loops;

pub use proof::UnprovenIndex;

/**
 * The most elements a list, a stack, a view or a text can hold. No data
 * stack reserves more than 2^36 bytes (`lymra_reserve_data_stacks` in the
 * runtime asks for that much and takes less where the system refuses),
 * growing past the end of one stops the program, and every element takes at
 * least a byte. Text the program keeps in itself, and its arguments, are far
 * shorter. So a loop over a sequence takes at most this many turns.
 */
const LONGEST_SEQUENCE: i128 = 1 << 36;

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
    let facts = facts::program_facts(program, &constants);
    program
        .functions
        .iter_mut()
        .enumerate()
        .flat_map(|(id, function)| analysis::optimize_function(function, id, &constants, &facts))
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

#[cfg(test)]
mod tests {
    use super::LONGEST_SEQUENCE;

    /// The bound has to follow the runtime: a data stack that reserved more would let a sequence pass it.
    #[test]
    fn the_longest_sequence_is_what_a_data_stack_reserves() {
        let runtime = include_str!("../codegen/runtime.h");
        let reservation = "size_t each = (size_t)1 << ";
        let shift: u32 = runtime
            .split_once(reservation)
            .and_then(|(_, after)| after.split_once(';'))
            .and_then(|(shift, _)| shift.parse().ok())
            .expect("the runtime reserves `(size_t)1 << N` bytes for each data stack");
        assert_eq!(LONGEST_SEQUENCE, 1 << shift);
    }
}
