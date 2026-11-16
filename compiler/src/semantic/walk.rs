/*!
 * Walks over the checked program that the whole-program passes share: the
 * expressions a statement computes, the calls and pops inside them, and the
 * blocks a statement holds.
 */

use std::collections::HashMap;

use crate::ir::{
    Argument, Block, Call, Expr, ExprKind, Function, LocalId, PlaceStep, PrintPart, Statement,
    text_values,
};
use crate::source::Span;

/// The expressions a statement computes outside the blocks it holds; for a call statement, the values it passes.
pub(super) fn statement_exprs(statement: &Statement) -> Vec<&Expr> {
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
        ExprKind::Text(parts) => {
            for value in text_values(parts) {
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

/// Every pool a statement inserts into outside the blocks it holds, with where.
pub(super) fn statement_inserts(statement: &Statement) -> Vec<(LocalId, Span)> {
    let mut inserts = Vec::new();
    for expr in statement_exprs(statement) {
        each_expr(expr, &mut |inner| {
            if let ExprKind::Insert { pool, .. } = inner.kind {
                inserts.push((pool, inner.span));
            }
        });
    }
    inserts
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

/// Which block declares each local, and which block holds each block. The function's body is block 0.
pub(super) struct Blocks {
    pub declared_in: HashMap<LocalId, usize>,
    parents: Vec<Option<usize>>,
}

impl Blocks {
    pub fn of(function: &Function<'_>) -> Self {
        let mut blocks = Self {
            declared_in: function
                .parameters
                .iter()
                .map(|&local| (local, 0))
                .collect(),
            parents: vec![None],
        };
        blocks.add(&function.body, 0);
        blocks
    }

    fn add(&mut self, block: &Block, id: usize) {
        for statement in &block.statements {
            if let Statement::Declare { local, .. } = statement {
                self.declared_in.insert(*local, id);
            }
            // A loop's variable and a case's bindings belong to the block they run in, and end with it.
            let declared: Vec<Vec<LocalId>> = match statement {
                Statement::ForRange { variable, .. } | Statement::ForEach { variable, .. } => {
                    vec![variable.iter().copied().collect()]
                }
                Statement::Match { cases, .. } => cases
                    .iter()
                    .map(|case| case.bindings.iter().map(|binding| binding.local).collect())
                    .collect(),
                _ => Vec::new(),
            };
            for (position, body) in statement_blocks(statement).into_iter().enumerate() {
                let child = self.parents.len();
                self.parents.push(Some(id));
                for &local in declared.get(position).into_iter().flatten() {
                    self.declared_in.insert(local, child);
                }
                self.add(body, child);
            }
        }
    }

    /// Whether `inner` is `outer` or lies inside it.
    pub fn within(&self, mut inner: usize, outer: usize) -> bool {
        loop {
            if inner == outer {
                return true;
            }
            match self.parents.get(inner).copied().flatten() {
                Some(parent) => inner = parent,
                None => return false,
            }
        }
    }
}
