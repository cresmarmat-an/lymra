/*!
 * What a loop changes, and how.
 *
 * A loop is analyzed without iterating to a fixed point. Instead, each
 * variable the loop changes gets bounds that hold at every point of every
 * iteration. When the only changes to a variable add or subtract values that
 * do not depend on it, as in `total = total + step`, one iteration changes it
 * by an amount within some interval, and a `for` loop runs a bounded number
 * of times, so its whole effect is bounded too. Every other change gives up,
 * and the variable is treated as holding any value of its type.
 */

use std::collections::{HashMap, HashSet};

use super::interval::Interval;
use crate::ir::{
    Argument, ArithmeticOperator, Block, Expr, ExprKind, LocalId, PrintPart, Statement,
};

/// How far an iteration has moved a variable from its value at the start of the iteration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change {
    /**
     * Only additive changes so far. `offset` holds the change up to this
     * point, and `extent` every change at any point before it, so it always
     * holds zero.
     */
    Additive { offset: Interval, extent: Interval },
    /// Some change is not additive, so nothing is known.
    Unknown,
}

impl Change {
    pub const NONE: Change = Change::Additive {
        offset: Interval::exact(0),
        extent: Interval::exact(0),
    };

    /// The change after adding `step` once more.
    #[must_use]
    pub fn add(self, step: Interval) -> Change {
        match self {
            Change::Additive { offset, extent } => {
                let offset = offset.add(step);
                Change::Additive {
                    offset,
                    extent: extent.join(offset),
                }
            }
            Change::Unknown => Change::Unknown,
        }
    }

    /// The change where two paths meet, as after an `if`.
    #[must_use]
    pub fn join(self, other: Change) -> Change {
        match (self, other) {
            (
                Change::Additive {
                    offset: first_offset,
                    extent: first_extent,
                },
                Change::Additive {
                    offset: second_offset,
                    extent: second_extent,
                },
            ) => Change::Additive {
                offset: first_offset.join(second_offset),
                extent: first_extent.join(second_extent),
            },
            _ => Change::Unknown,
        }
    }

    /// The change at every point of this iteration, which bounds the change of any one iteration.
    pub fn extent(self) -> Option<Interval> {
        match self {
            Change::Additive { extent, .. } => Some(extent),
            Change::Unknown => None,
        }
    }
}

/// A variable that a loop being probed changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Followed {
    pub change: Change,
    /**
     * Bounds that hold for the variable at every point of the loop, as far as
     * they are known yet. After the variable is assigned, the walk goes back
     * to these, because a bound narrowed by an earlier condition no longer
     * holds.
     */
    pub loop_wide: Option<Interval>,
}

/// The variables a loop being probed changes, with what an iteration does to each.
pub(super) type Changes = HashMap<LocalId, Followed>;

/// Joins what two paths did to the followed variables, as after an `if`.
pub(super) fn join_changes(first: &Changes, second: &Changes) -> Changes {
    first
        .iter()
        .map(|(&local, followed)| {
            let change = match second.get(&local) {
                Some(other) => followed.change.join(other.change),
                None => Change::Unknown,
            };
            (
                local,
                Followed {
                    change,
                    ..*followed
                },
            )
        })
        .collect()
}

/// An upper bound on how many times `for _ in range(start, end)` runs.
pub(super) fn trip_count(start: Interval, end: Interval) -> i128 {
    end.high.saturating_sub(start.low).max(0)
}

/// The locals a block changes and that live outside it: assigned, or passed with `variable`.
pub(super) fn changed_outside(block: &Block) -> HashSet<LocalId> {
    let mut changed = HashSet::new();
    let mut declared = HashSet::new();
    collect_block(block, &mut changed, &mut declared);
    changed.retain(|local| !declared.contains(local));
    changed
}

fn collect_block(block: &Block, changed: &mut HashSet<LocalId>, declared: &mut HashSet<LocalId>) {
    for statement in &block.statements {
        collect_statement(statement, changed, declared);
    }
}

fn collect_statement(
    statement: &Statement,
    changed: &mut HashSet<LocalId>,
    declared: &mut HashSet<LocalId>,
) {
    match statement {
        Statement::Declare { local, value } => {
            declared.insert(*local);
            collect_references(value, changed);
        }
        Statement::Assign { target, value } => {
            changed.insert(target.local);
            for index in &target.indexes {
                collect_references(&index.value, changed);
            }
            collect_references(value, changed);
        }
        Statement::Discard(value)
        | Statement::Assert { condition: value }
        | Statement::Return(Some(value)) => {
            collect_references(value, changed);
        }
        Statement::Call(call) => collect_call_references(&call.arguments, changed),
        Statement::Print(parts) => {
            for part in parts {
                if let PrintPart::Value { value, .. } = part {
                    collect_references(value, changed);
                }
            }
        }
        Statement::If {
            branches,
            otherwise,
        } => {
            for branch in branches {
                collect_references(&branch.condition, changed);
                collect_block(&branch.body, changed, declared);
            }
            if let Some(otherwise) = otherwise {
                collect_block(otherwise, changed, declared);
            }
        }
        Statement::While { condition, body } => {
            collect_references(condition, changed);
            collect_block(body, changed, declared);
        }
        Statement::ForRange {
            variable,
            start,
            end,
            body,
        } => {
            declared.extend(variable);
            collect_references(start, changed);
            collect_references(end, changed);
            collect_block(body, changed, declared);
        }
        Statement::ForEach {
            variable,
            array,
            body,
            ..
        } => {
            declared.extend(variable);
            collect_references(array, changed);
            collect_block(body, changed, declared);
        }
        Statement::Return(None) | Statement::Break | Statement::Continue => {}
    }
}

fn collect_call_references(arguments: &[Argument], changed: &mut HashSet<LocalId>) {
    for argument in arguments {
        match argument {
            Argument::Reference(local) => {
                changed.insert(*local);
            }
            Argument::Value(value) => collect_references(value, changed),
        }
    }
}

/// Collects the locals an expression passes with `variable`, which the called function may change.
pub(super) fn collect_references(expr: &Expr, changed: &mut HashSet<LocalId>) {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Local(_) | ExprKind::Constant(_) => {}
        ExprKind::Negate { operand, .. }
        | ExprKind::Not(operand)
        | ExprKind::Convert { operand, .. }
        | ExprKind::Fill(operand)
        | ExprKind::Length(operand) => {
            collect_references(operand, changed);
        }
        ExprKind::Arithmetic { left, right, .. }
        | ExprKind::Compare { left, right, .. }
        | ExprKind::Logic { left, right, .. }
        | ExprKind::Index {
            array: left,
            index: right,
            ..
        } => {
            collect_references(left, changed);
            collect_references(right, changed);
        }
        ExprKind::Call(call) => collect_call_references(&call.arguments, changed),
    }
}

/// A term of a sum: an expression and whether it is added or subtracted.
pub(super) struct Term<'expr> {
    pub expr: &'expr Expr,
    pub positive: bool,
}

/**
 * Reads `value` as `local` plus other terms that do not mention `local`, as in
 * `total + index * 3 + round`. Returns those other terms, or `None` when the
 * value has any other shape, such as `total * 2` or `limit - total`.
 */
pub(super) fn additive_terms(value: &Expr, local: LocalId) -> Option<Vec<Term<'_>>> {
    let mut terms = Vec::new();
    let mut found_self = false;
    flatten(value, local, true, &mut terms, &mut found_self)?;
    found_self.then_some(terms)
}

fn flatten<'expr>(
    expr: &'expr Expr,
    local: LocalId,
    positive: bool,
    terms: &mut Vec<Term<'expr>>,
    found_self: &mut bool,
) -> Option<()> {
    match &expr.kind {
        ExprKind::Local(id) if *id == local => {
            // The variable itself has to appear once, added, so the result is the old value plus a change.
            if *found_self || !positive {
                return None;
            }
            *found_self = true;
            Some(())
        }
        ExprKind::Arithmetic {
            operator: ArithmeticOperator::Add,
            left,
            right,
            ..
        } => {
            flatten(left, local, positive, terms, found_self)?;
            flatten(right, local, positive, terms, found_self)
        }
        ExprKind::Arithmetic {
            operator: ArithmeticOperator::Subtract,
            left,
            right,
            ..
        } => {
            flatten(left, local, positive, terms, found_self)?;
            flatten(right, local, !positive, terms, found_self)
        }
        _ if expr.mentions(local) => None,
        _ => {
            terms.push(Term { expr, positive });
            Some(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IntegerType, Type, Value};
    use crate::source::Span;

    fn local(id: LocalId) -> Expr {
        Expr {
            kind: ExprKind::Local(id),
            ty: Type::Integer(IntegerType::Integer64),
            span: Span::empty(0),
        }
    }

    fn number(value: i128) -> Expr {
        Expr {
            kind: ExprKind::Literal(Value::Integer {
                value,
                ty: IntegerType::Integer64,
            }),
            ty: Type::Integer(IntegerType::Integer64),
            span: Span::empty(0),
        }
    }

    fn arithmetic(operator: ArithmeticOperator, left: Expr, right: Expr) -> Expr {
        Expr {
            kind: ExprKind::Arithmetic {
                operator,
                left: Box::new(left),
                right: Box::new(right),
                checked: true,
            },
            ty: Type::Integer(IntegerType::Integer64),
            span: Span::empty(0),
        }
    }

    #[test]
    fn sums_that_add_to_the_variable_are_additive() {
        let total = 0;
        let step = arithmetic(ArithmeticOperator::Multiply, local(1), number(3));
        let value = arithmetic(
            ArithmeticOperator::Add,
            arithmetic(ArithmeticOperator::Add, local(total), step),
            local(2),
        );
        let terms = additive_terms(&value, total).expect("the sum is additive");
        assert_eq!(terms.len(), 2);
        assert!(terms.iter().all(|term| term.positive));

        let subtracted = arithmetic(ArithmeticOperator::Subtract, local(total), local(1));
        let terms = additive_terms(&subtracted, total).expect("subtracting is additive");
        assert!(!terms[0].positive);
    }

    #[test]
    fn other_shapes_are_not_additive() {
        let total = 0;
        let doubled = arithmetic(ArithmeticOperator::Multiply, local(total), number(2));
        assert!(additive_terms(&doubled, total).is_none());
        let reversed = arithmetic(ArithmeticOperator::Subtract, number(10), local(total));
        assert!(additive_terms(&reversed, total).is_none());
        let twice = arithmetic(ArithmeticOperator::Add, local(total), local(total));
        assert!(additive_terms(&twice, total).is_none());
        assert!(additive_terms(&local(1), total).is_none());
    }

    #[test]
    fn changes_accumulate_and_remember_every_point() {
        let change = Change::NONE
            .add(Interval::spanning(1, 3))
            .add(Interval::spanning(-5, -5));
        assert_eq!(change.extent(), Some(Interval::spanning(-4, 3)));
        assert_eq!(Change::NONE.join(Change::Unknown).extent(), None);
    }
}
