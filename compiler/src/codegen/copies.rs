/*!
 * Loop bodies the code generator writes out twice, as two versions of one
 * loop: a loop run in blocks of turns (see `strips`) and a loop that pushes
 * into room reserved for it (see `reservations`). Each version runs on its
 * own, so a body can be copied when nothing it does depends on there being
 * one copy: it is small, and it keeps nothing on a data stack, which would
 * get data stacks of its own in each copy.
 */

use super::Storage;
use crate::ir::walk::{each_expr, statement_blocks, statement_exprs};
use crate::ir::{Block, Expr, ExprKind, Function, LocalId, Statement};

/// The most statements and expressions in a body that gets written out twice.
const LARGEST_BODY: usize = 256;

/// Whether `body`, the body of a loop in `function`, can be written out twice.
pub(super) fn can_copy(function: &Function<'_>, body: &Block) -> bool {
    body_size(body) <= LARGEST_BODY && !keeps_storage(function, body)
}

/// Visits every statement in `block`, in the blocks it holds too.
pub(super) fn each_statement<'a>(block: &'a Block, visit: &mut impl FnMut(&'a Statement)) {
    for statement in &block.statements {
        visit(statement);
        for inner in statement_blocks(statement) {
            each_statement(inner, visit);
        }
    }
}

/// Visits every expression `block` computes, in the blocks it holds too.
pub(super) fn each_body_expr<'a>(block: &'a Block, visit: &mut impl FnMut(&'a Expr)) {
    each_statement(block, &mut |statement| {
        for expr in statement_exprs(statement) {
            each_expr(expr, visit);
        }
    });
}

/// Whether a `break` in `block` leaves the loop whose body it is, rather than a loop inside it.
pub(super) fn breaks_out(block: &Block) -> bool {
    leaves_loop(block, |statement| matches!(statement, Statement::Break))
}

/// Whether a `break` or `continue` in `block` ends a turn of the loop whose body it is before the turn's last statement.
pub(super) fn ends_turns_early(block: &Block) -> bool {
    leaves_loop(block, |statement| {
        matches!(statement, Statement::Break | Statement::Continue)
    })
}

/// Whether a statement `leaves` matches in `block` applies to the loop whose body it is, rather than a loop inside it.
fn leaves_loop(block: &Block, leaves: fn(&Statement) -> bool) -> bool {
    block.statements.iter().any(|statement| match statement {
        Statement::While { .. } | Statement::ForRange { .. } | Statement::ForEach { .. } => false,
        _ if leaves(statement) => true,
        _ => statement_blocks(statement)
            .into_iter()
            .any(|inner| leaves_loop(inner, leaves)),
    })
}

/// How many statements and expressions `block` holds.
fn body_size(block: &Block) -> usize {
    let mut size = 0;
    each_statement(block, &mut |_| size += 1);
    each_body_expr(block, &mut |_| size += 1);
    size
}

/**
 * Whether running `block` can put anything on a data stack: a binding that
 * keeps a list or text, a value built without a binding, a list or text
 * given a new value, or one returned. Reading such a value puts nothing
 * there.
 */
fn keeps_storage(function: &Function<'_>, block: &Block) -> bool {
    let keeps = |local: LocalId| Storage::of(&function.locals[local].ty) != Storage::None;
    let mut found = false;
    each_statement(block, &mut |statement| {
        found |= match statement {
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
    });
    each_body_expr(block, &mut |expr| found |= builds_storage(expr));
    found
}

/// Whether `expr` itself builds a list or text: anything that has one, other than a read of a value that keeps it.
fn builds_storage(expr: &Expr) -> bool {
    let reads = matches!(
        expr.kind,
        ExprKind::Local(_)
            | ExprKind::Constant(_)
            | ExprKind::Literal(_)
            | ExprKind::Field { .. }
            | ExprKind::Index { .. }
    );
    !reads && Storage::of(&expr.ty) != Storage::None
}
