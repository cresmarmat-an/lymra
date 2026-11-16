/*!
 * Walks over the checked program: the expressions a statement computes, the
 * expressions inside an expression, and the blocks a statement holds. The
 * whole-program passes of the checker and the code generator share them.
 */

use super::{Argument, Block, Call, Expr, ExprKind, PlaceStep, PrintPart, Statement, text_values};

/// The expressions a statement computes outside the blocks it holds; for a call statement, the values it passes.
pub fn statement_exprs(statement: &Statement) -> Vec<&Expr> {
    match statement {
        Statement::Declare { value, .. }
        | Statement::Discard(value)
        | Statement::Push { value, .. }
        | Statement::Remove { handle: value, .. }
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
                PlaceStep::Slot(slot) => Some(&slot.handle),
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
        Statement::Try(attempt) => attempt.parts().collect(),
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
pub fn each_expr<'a>(expr: &'a Expr, visit: &mut impl FnMut(&'a Expr)) {
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
        ExprKind::Text(parts) => {
            for value in text_values(parts) {
                each_expr(value, visit);
            }
        }
        ExprKind::Try(attempt) => {
            for part in attempt.parts() {
                each_expr(part, visit);
            }
        }
        ExprKind::System { arguments, .. } => {
            for argument in arguments {
                each_expr(argument, visit);
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
        | ExprKind::Insert { value: operand, .. }
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
        }
        | ExprKind::Slot {
            pool: left,
            handle: right,
        } => {
            each_expr(left, visit);
            each_expr(right, visit);
        }
    }
}

/// The blocks a statement holds, in order.
pub fn statement_blocks(statement: &Statement) -> Vec<&Block> {
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
