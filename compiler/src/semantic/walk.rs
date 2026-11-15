/*!
 * Walks over the checked program that the whole-program passes share: the
 * calls a statement makes, and the blocks it holds.
 */

use crate::ir::{Argument, Block, Call, Expr, ExprKind, PlaceStep, PrintPart, Statement};

/// Every call a statement makes outside the blocks it holds.
pub(super) fn statement_calls<'a>(statement: &'a Statement, calls: &mut Vec<&'a Call>) {
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
        | Statement::Match { value, .. } => expr_calls(value, calls),
        Statement::Assign { target, value } => {
            for step in &target.steps {
                if let PlaceStep::Index(index) = step {
                    expr_calls(&index.value, calls);
                }
            }
            expr_calls(value, calls);
        }
        Statement::Call(call) => call_calls(call, calls),
        Statement::Print(parts) => {
            for part in parts {
                if let PrintPart::Value { value, .. } = part {
                    expr_calls(value, calls);
                }
            }
        }
        Statement::If { branches, .. } => {
            for branch in branches {
                expr_calls(&branch.condition, calls);
            }
        }
        Statement::ForRange { start, end, .. } => {
            expr_calls(start, calls);
            expr_calls(end, calls);
        }
        Statement::Return(None) | Statement::Break | Statement::Continue => {}
    }
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

/// `call` and every call inside its arguments.
fn call_calls<'a>(call: &'a Call, calls: &mut Vec<&'a Call>) {
    calls.push(call);
    for argument in &call.arguments {
        if let Argument::Value(value) = argument {
            expr_calls(value, calls);
        }
    }
}

/// Every call inside an expression.
pub(super) fn expr_calls<'a>(expr: &'a Expr, calls: &mut Vec<&'a Call>) {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Local(_) | ExprKind::Constant(_) | ExprKind::EmptyList => {
        }
        ExprKind::Call(call) => call_calls(call, calls),
        ExprKind::Negate { operand, .. }
        | ExprKind::Not(operand)
        | ExprKind::Convert { operand, .. }
        | ExprKind::Fill(operand)
        | ExprKind::Length(operand)
        | ExprKind::View(operand)
        | ExprKind::Bytes(operand)
        | ExprKind::Field {
            object: operand, ..
        } => expr_calls(operand, calls),
        ExprKind::Slice { items, start, end } => {
            expr_calls(items, calls);
            expr_calls(start, calls);
            expr_calls(end, calls);
        }
        ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => {
            for field in fields {
                expr_calls(&field.value, calls);
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
            expr_calls(left, calls);
            expr_calls(right, calls);
        }
    }
}
