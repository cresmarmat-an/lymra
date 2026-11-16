/*!
 * What the whole-program passes of the checker gather from a function: the
 * calls, pops and inserts a statement makes, and which block declares each
 * local.
 */

use std::collections::HashMap;

use crate::ir::walk::{each_expr, statement_blocks, statement_exprs};
use crate::ir::{Block, Call, Expr, ExprKind, Function, LocalId, Statement};
use crate::source::Span;

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
