/*!
 * Range loops that run in blocks of turns, which C compilers make fast.
 *
 * In `for index in range(0, count)`, a body that takes `index % 7` keeps the
 * C compiler from unrolling the loop or finding a formula for it: it sees a
 * division on every turn. Run in blocks of 7 turns whose first index is a
 * multiple of 7, `index % 7` is the turn's place in its block. The code
 * generator writes such a loop as an outer loop over whole blocks, an inner
 * loop over the places in a block, with that place standing for the
 * remainder, and the plain loop for the turns left after the last whole
 * block. The C compiler can unroll the inner loop, after which the
 * remainder is a constant.
 *
 * The restatement computes exactly what the loop does: the same turns in the
 * same order, with the same values. It needs a counter that never goes
 * below zero, so the remainder of every index is its place in its block, and
 * a body with no `break` for the loop, which would leave only the inner
 * loop, that can be written out twice (see `copies`).
 */

use super::copies::{breaks_out, can_copy, each_body_expr};
use crate::ast::OptimizeMode;
use crate::ir::{
    ArithmeticOperator, Block, Expr, ExprKind, Function, IntegerType, LocalId, Program, Value,
};

/// The most turns in a block: the inner loop has to stay small enough for the C compiler to unroll it.
const LARGEST_BLOCK: i128 = 16;

/// A range loop run in blocks of `size` turns, where `counter % size` is the place of a turn in its block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Strip {
    pub counter: LocalId,
    /// The counter's type.
    pub ty: IntegerType,
    pub size: i128,
}

/**
 * How `for counter in range(start, …)` with `body` runs in blocks, if it
 * can: its counter starts at a multiple of the block size that is not below
 * zero, and the body takes the counter's remainder by numbers that divide
 * the block size.
 */
pub(super) fn strip(
    program: &Program<'_>,
    function: &Function<'_>,
    counter: Option<LocalId>,
    start: &Expr,
    body: &Block,
) -> Option<Strip> {
    // A program built for size or for debugging keeps its loops as written.
    if !matches!(program.optimize, None | Some(OptimizeMode::Speed)) {
        return None;
    }
    let counter = counter?;
    let ty = function.locals[counter].ty.as_integer()?;
    let start = integer_constant(program, start)?;
    let mut size = 1;
    each_body_expr(body, &mut |expr| {
        if let Some(divisor) = block_divisor(program, counter, expr) {
            size = least_common_multiple(size, divisor);
        }
    });
    let fits = (2..=LARGEST_BLOCK).contains(&size)
        && start >= 0
        && start % size == 0
        && !breaks_out(body)
        && can_copy(function, body);
    fits.then_some(Strip { counter, ty, size })
}

/**
 * The divisor of `expr` when it is the remainder of `counter` by a number
 * that can divide a block size: one the optimizer proved cannot fail, so
 * the restatement leaves every check that stays where it was.
 */
pub(super) fn block_divisor(program: &Program<'_>, counter: LocalId, expr: &Expr) -> Option<i128> {
    let ExprKind::Arithmetic {
        operator: ArithmeticOperator::Remainder,
        left,
        right,
        checked: false,
    } = &expr.kind
    else {
        return None;
    };
    if !matches!(left.kind, ExprKind::Local(local) if local == counter) {
        return None;
    }
    integer_constant(program, right).filter(|divisor| (2..=LARGEST_BLOCK).contains(divisor))
}

/// The value of `expr` when it is an integer known while compiling: a number written out, or a named constant.
fn integer_constant(program: &Program<'_>, expr: &Expr) -> Option<i128> {
    let value = match &expr.kind {
        ExprKind::Literal(value) => value,
        ExprKind::Constant(id) => &program.constants[*id].value,
        _ => return None,
    };
    match value {
        Value::Integer { value, .. } => Some(*value),
        Value::Float { .. } | Value::Boolean(_) | Value::Text(_) => None,
    }
}

const fn least_common_multiple(left: i128, right: i128) -> i128 {
    let (mut larger, mut smaller) = (left, right);
    while smaller != 0 {
        (larger, smaller) = (smaller, larger % smaller);
    }
    left / larger * right
}

#[cfg(test)]
mod tests {
    use super::least_common_multiple;

    #[test]
    fn least_common_multiples() {
        assert_eq!(least_common_multiple(1, 7), 7);
        assert_eq!(least_common_multiple(3, 5), 15);
        assert_eq!(least_common_multiple(4, 6), 12);
        assert_eq!(least_common_multiple(8, 8), 8);
        assert_eq!(least_common_multiple(15, 16), 240);
    }
}
