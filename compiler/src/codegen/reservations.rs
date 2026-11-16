/*!
 * `for` loops that push onto a list once on every turn, which get room for
 * every element before the first turn. A loop over a range knows how many
 * turns it takes when it starts, and so does a loop over an array or a list,
 * which goes over the elements it holds then.
 *
 * A `push` checks that its list is the newest value on its data stack and
 * that memory is committed for one more element, and moves the top of the
 * stack. In a loop that does nothing but fill a list, those loads and stores
 * keep the C compiler from turning the loop into a fill of memory. When the
 * body pushes onto the same list exactly once on every turn, the number of
 * turns is the number of elements, so the code generator writes two
 * versions of the loop: one that first reserves room for that many elements
 * and pushes into it with no check, then frees the room it did not take,
 * and the loop as written, which runs when the room cannot be had and stops
 * the program at the push where room runs out, as it always would.
 *
 * The body pushes exactly once on every turn when the push is one of its
 * own statements, no `break` or `continue` ends a turn before it, and no
 * `return` or `try` leaves the function. Nothing else may change the list's
 * stack: no other push or pop, no new value for the whole list, and no call
 * that gets the list with `variable`. A recursive call puts what it builds
 * above the reserved room and frees it before it returns. The list's
 * elements hold no text, which another data stack keeps, and the body can
 * be written out twice (see `copies`).
 */

use super::Storage;
use super::copies::{can_copy, each_body_expr, each_statement, ends_turns_early};
use crate::ast::OptimizeMode;
use crate::ir::{Argument, Block, Call, ExprKind, Function, LocalId, Program, Statement};

/// The list that a `for` loop with `body` pushes onto once on every turn, if it reserves room for it first.
pub(super) fn reserved_list(
    program: &Program<'_>,
    function: &Function<'_>,
    body: &Block,
) -> Option<LocalId> {
    // A program built for size or for debugging keeps its loops as written.
    if !matches!(program.optimize, None | Some(OptimizeMode::Speed)) {
        return None;
    }
    let mut pushed = body
        .statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::Push { list, .. } => Some(*list),
            _ => None,
        });
    let list = pushed.next()?;
    let fits = pushed.next().is_none()
        && Storage::of(&function.locals[list].ty) == Storage::Elements
        && changes(body, list) == 1
        && !leaves_function(body)
        && !ends_turns_early(body)
        && can_copy(function, body);
    fits.then_some(list)
}

/// How many statements and expressions in `body` can change where `list` ends on its data stack.
fn changes(body: &Block, list: LocalId) -> usize {
    let mut count = 0;
    each_statement(body, &mut |statement| {
        count += usize::from(match statement {
            Statement::Push { list: pushed, .. } => *pushed == list,
            Statement::Assign { target, .. } => target.local == list && target.steps.is_empty(),
            Statement::Call(call) => passes(call, list),
            _ => false,
        });
    });
    each_body_expr(body, &mut |expr| {
        count += usize::from(match &expr.kind {
            ExprKind::Pop(popped) => *popped == list,
            ExprKind::Call(call) => passes(call, list),
            _ => false,
        });
    });
    count
}

/// Whether `call` gets `list` with `variable`, so it can push onto it.
fn passes(call: &Call, list: LocalId) -> bool {
    call.arguments
        .iter()
        .any(|argument| matches!(argument, Argument::Reference { local, .. } if *local == list))
}

/// Whether running `body` can leave its function: a `return`, which `fail` is too, or a `try` that passes a failure on.
fn leaves_function(body: &Block) -> bool {
    let mut leaves = false;
    each_statement(body, &mut |statement| {
        leaves |= matches!(statement, Statement::Return(_) | Statement::Try(_));
    });
    each_body_expr(body, &mut |expr| {
        leaves |= matches!(expr.kind, ExprKind::Try(_));
    });
    leaves
}
