/*!
 * Walks over the checked program that the whole-program passes share: the
 * expressions a statement computes, the calls and pops inside them, and the
 * blocks a statement holds.
 */

use crate::ir::{Argument, Block, Call, Expr, ExprKind, LocalId, PlaceStep, PrintPart, Statement};
use crate::source::Span;

/// The expressions a statement computes outside the blocks it holds; for a call statement, the values it passes.
fn statement_exprs(statement: &Statement) -> Vec<&Expr> {
    match statement {
        Statement::Declare { value, .. }
        | Statement::Discard(value)
        | Statement::Push { value, .. }
        | Statement::Return(Some(value))
        | Statement::Assert { condition: value }
        | Statement::While {
            condition: value, ..
        }
        | Statement::ForEach { array: value, .. }
        | Statement::Match { value, .. } => vec![value],
        Statement::Assign { target, value } => target
            .steps
            .iter()
            .filter_map(|step| match step {
                PlaceStep::Index(index) => Some(&index.value),
                PlaceStep::Field(_) => None,
            })
            .chain([value])
            .collect(),
        Statement::Call(call) => argument_values(call).collect(),
        Statement::Print(parts) => parts
            .iter()
            .filter_map(|part| match part {
                PrintPart::Value { value, .. } => Some(value),
                PrintPart::Text(_) => None,
            })
            .collect(),
        Statement::If { branches, .. } => branches.iter().map(|branch| &branch.condition).collect(),
        Statement::ForRange { start, end, .. } => vec![start, end],
        Statement::Return(None) | Statement::Break | Statement::Continue => Vec::new(),
    }
}

/// The values a call passes, leaving out the bindings it passes with `variable`.
fn argument_values(call: &Call) -> impl Iterator<Item = &Expr> {
    call.arguments.iter().filter_map(|argument| match argument {
        Argument::Value(value) => Some(value),
        Argument::Reference { .. } => None,
    })
}

/// Visits `expr` and every expression inside it, the values passed to calls included.
pub(super) fn each_expr<'a>(expr: &'a Expr, visit: &mut impl FnMut(&'a Expr)) {
    visit(expr);
    match &expr.kind {
        ExprKind::Literal(_)
        | ExprKind::Local(_)
        | ExprKind::Constant(_)
        | ExprKind::EmptyList
        | ExprKind::Pop(_) => {}
        ExprKind::Call(call) => {
            for value in argument_values(call) {
                each_expr(value, visit);
            }
        }
        ExprKind::Negate { operand, .. }
        | ExprKind::Not(operand)
        | ExprKind::Convert { operand, .. }
        | ExprKind::Fill(operand)
        | ExprKind::Length(operand)
        | ExprKind::View(operand)
        | ExprKind::Bytes(operand)
        | ExprKind::Top(operand)
        | ExprKind::Field {
            object: operand, ..
        } => each_expr(operand, visit),
        ExprKind::Slice { items, start, end } => {
            each_expr(items, visit);
            each_expr(start, visit);
            each_expr(end, visit);
        }
        ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => {
            for field in fields {
                each_expr(&field.value, visit);
            }
        }
        ExprKind::Arithmetic { left, right, .. }
        | ExprKind::Compare { left, right, .. }
        | ExprKind::Logic { left, right, .. }
        | ExprKind::Wrapping { left, right, .. }
        | ExprKind::Index {
            array: left,
            index: right,
            ..
        } => {
            each_expr(left, visit);
            each_expr(right, visit);
        }
    }
}

/// Every call inside an expression.
fn expr_calls<'a>(expr: &'a Expr, calls: &mut Vec<&'a Call>) {
    each_expr(expr, &mut |inner| {
        if let ExprKind::Call(call) = &inner.kind {
            calls.push(call);
        }
    });
}

/// Every call a statement makes outside the blocks it holds.
pub(super) fn statement_calls<'a>(statement: &'a Statement, calls: &mut Vec<&'a Call>) {
    if let Statement::Call(call) = statement {
        calls.push(call);
    }
    for expr in statement_exprs(statement) {
        expr_calls(expr, calls);
    }
}

/// Every stack a statement pops outside the blocks it holds, with where.
pub(super) fn statement_pops(statement: &Statement) -> Vec<(LocalId, Span)> {
    let mut pops = Vec::new();
    for expr in statement_exprs(statement) {
        each_expr(expr, &mut |inner| {
            if let ExprKind::Pop(stack) = inner.kind {
                pops.push((stack, inner.span));
            }
        });
    }
    pops
}

/// The blocks a statement holds, in order.
pub(super) fn statement_blocks(statement: &Statement) -> Vec<&Block> {
    match statement {
        Statement::If {
            branches,
            otherwise,
        } => branches
            .iter()
            .map(|branch| &branch.body)
            .chain(otherwise)
            .collect(),
        Statement::While { body, .. }
        | Statement::ForRange { body, .. }
        | Statement::ForEach { body, .. } => vec![body],
        Statement::Match { cases, .. } => cases.iter().map(|case| &case.body).collect(),
        _ => Vec::new(),
    }
}
