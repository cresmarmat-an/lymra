/*!
 * Walking the syntax tree.
 *
 * A pass implements `Visitor`, overrides the `visit_*` methods for the nodes
 * it checks, and calls the matching `walk_*` function to continue into the
 * node's children.
 */

use super::{
    ArgumentKind, Block, Expr, ExprKind, Item, ItemKind, Module, Statement, StatementKind, TextPart,
};

pub trait Visitor<'src>: Sized {
    fn visit_item(&mut self, item: &Item<'src>) {
        walk_item(self, item);
    }

    fn visit_statement(&mut self, statement: &Statement<'src>) {
        walk_statement(self, statement);
    }

    fn visit_expr(&mut self, expr: &Expr<'src>) {
        walk_expr(self, expr);
    }
}

pub fn walk_module<'src>(visitor: &mut impl Visitor<'src>, module: &Module<'src>) {
    for item in &module.items {
        visitor.visit_item(item);
    }
}

pub fn walk_item<'src>(visitor: &mut impl Visitor<'src>, item: &Item<'src>) {
    match &item.kind {
        ItemKind::Constant(binding) => visitor.visit_expr(&binding.value),
        ItemKind::Function(function) => walk_block(visitor, &function.body),
        ItemKind::Use(_) | ItemKind::Type(_) | ItemKind::Choice(_) => {}
    }
}

pub fn walk_block<'src>(visitor: &mut impl Visitor<'src>, block: &Block<'src>) {
    for statement in &block.statements {
        visitor.visit_statement(statement);
    }
}

pub fn walk_statement<'src>(visitor: &mut impl Visitor<'src>, statement: &Statement<'src>) {
    match &statement.kind {
        StatementKind::Binding(binding) => visitor.visit_expr(&binding.value),
        StatementKind::Assignment { target, value } => {
            visitor.visit_expr(target);
            visitor.visit_expr(value);
        }
        StatementKind::Expression(expr)
        | StatementKind::Assert(expr)
        | StatementKind::Return(Some(expr)) => {
            visitor.visit_expr(expr);
        }
        StatementKind::If(statement) => {
            for branch in &statement.branches {
                visitor.visit_expr(&branch.condition);
                walk_block(visitor, &branch.body);
            }
            if let Some(otherwise) = &statement.otherwise {
                walk_block(visitor, otherwise);
            }
        }
        StatementKind::While { condition, body } => {
            visitor.visit_expr(condition);
            walk_block(visitor, body);
        }
        StatementKind::For { iterable, body, .. } => {
            visitor.visit_expr(iterable);
            walk_block(visitor, body);
        }
        StatementKind::Match { scrutinee, arms } => {
            visitor.visit_expr(scrutinee);
            for arm in arms {
                walk_block(visitor, &arm.body);
            }
        }
        StatementKind::Return(None) | StatementKind::Break | StatementKind::Continue => {}
    }
}

pub fn walk_expr<'src>(visitor: &mut impl Visitor<'src>, expr: &Expr<'src>) {
    match &expr.kind {
        ExprKind::Integer(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Name(_) => {}
        ExprKind::Text(parts) => {
            for part in parts {
                if let TextPart::Interpolation(interpolated) = part {
                    visitor.visit_expr(interpolated);
                }
            }
        }
        ExprKind::Unary { operand, .. } => visitor.visit_expr(operand),
        ExprKind::Binary { left, right, .. } => {
            visitor.visit_expr(left);
            visitor.visit_expr(right);
        }
        ExprKind::Call(call) => {
            visitor.visit_expr(&call.callee);
            for argument in &call.arguments {
                match &argument.kind {
                    ArgumentKind::Positional(value) | ArgumentKind::Mutable(value) => {
                        visitor.visit_expr(value)
                    }
                    ArgumentKind::Named { value, .. } => visitor.visit_expr(value),
                }
            }
            if let Some(block) = &call.block {
                walk_block(visitor, &block.body);
            }
        }
        ExprKind::Field { object, .. } => visitor.visit_expr(object),
        ExprKind::Index { object, index } => {
            visitor.visit_expr(object);
            visitor.visit_expr(index);
        }
    }
}
