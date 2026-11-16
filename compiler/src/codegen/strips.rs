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
 * a body that is safe to write out twice: one with no `break` for the loop,
 * which would leave only the inner loop, and one that keeps nothing on a data
 * stack, which would get data stacks of its own in each copy.
 */

use super::Storage;
use crate::ast::OptimizeMode;
use crate::ir::walk::{each_expr, statement_blocks, statement_exprs};
use crate::ir::{
    ArithmeticOperator, Block, Expr, ExprKind, Function, IntegerType, LocalId, Program, Statement,
    Value,
};

/// The most turns in a block: the inner loop has to stay small enough for the C compiler to unroll it.
const LARGEST_BLOCK: i128 = 16;

/// The most statements and expressions in a body that gets written out twice.
const LARGEST_BODY: usize = 256;

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
        && body_size(body) <= LARGEST_BODY
        && !breaks_out(body)
        && !keeps_storage(function, body);
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

/// Visits every expression `block` computes, in the blocks it holds too.
fn each_body_expr<'a>(block: &'a Block, visit: &mut impl FnMut(&'a Expr)) {
    for statement in &block.statements {
        for expr in statement_exprs(statement) {
            each_expr(expr, visit);
        }
        for inner in statement_blocks(statement) {
            each_body_expr(inner, visit);
        }
    }
}

/// How many statements and expressions `block` holds.
fn body_size(block: &Block) -> usize {
    let mut size = 0;
    each_body_expr(block, &mut |_| size += 1);
    size + statement_count(block)
}

fn statement_count(block: &Block) -> usize {
    block
        .statements
        .iter()
        .map(|statement| {
            1 + statement_blocks(statement)
                .into_iter()
                .map(statement_count)
                .sum::<usize>()
        })
        .sum()
}

/// Whether a `break` in `block` leaves the loop whose body it is, rather than a loop inside it.
fn breaks_out(block: &Block) -> bool {
    block.statements.iter().any(|statement| match statement {
        Statement::Break => true,
        Statement::While { .. } | Statement::ForRange { .. } | Statement::ForEach { .. } => false,
        _ => statement_blocks(statement).into_iter().any(breaks_out),
    })
}

/**
 * Whether running `block` can put anything on a data stack: a binding that
 * keeps a list or text, a value built without a binding, a list or text
 * given a new value, or one returned. Reading such a value puts nothing
 * there.
 */
fn keeps_storage(function: &Function<'_>, block: &Block) -> bool {
    let keeps = |local: LocalId| Storage::of(&function.locals[local].ty) != Storage::None;
    block.statements.iter().any(|statement| {
        let binds = match statement {
            Statement::Declare { local, .. } => keeps(*local),
            Statement::ForEach { variable, .. } => variable.is_some_and(keeps),
            Statement::Match { cases, .. } => cases
                .iter()
                .flat_map(|case| &case.bindings)
                .any(|binding| keeps(binding.local)),
            Statement::Assign { value, .. } | Statement::Return(Some(value)) => {
                Storage::of(&value.ty) != Storage::None
            }
            _ => false,
        };
        binds
            || statement_exprs(statement).into_iter().any(builds_storage)
            || statement_blocks(statement)
                .into_iter()
                .any(|inner| keeps_storage(function, inner))
    })
}

/// Whether computing `expr` builds a list or text: anything that has one, other than a read of a value that keeps it.
fn builds_storage(expr: &Expr) -> bool {
    let mut builds = false;
    each_expr(expr, &mut |inner| {
        let reads = matches!(
            inner.kind,
            ExprKind::Local(_)
                | ExprKind::Constant(_)
                | ExprKind::Literal(_)
                | ExprKind::Field { .. }
                | ExprKind::Index { .. }
        );
        builds |= !reads && Storage::of(&inner.ty) != Storage::None;
    });
    builds
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
