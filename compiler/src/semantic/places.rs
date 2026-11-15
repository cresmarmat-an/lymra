//! The targets of assignments: a binding, or an element or a field inside one, as in `grid[row].cells[column]`.

use super::{CheckResult, Checker};
use crate::ast;
use crate::ir::{self, Type};
use crate::source::Span;

/// One step into a value in an assignment target.
pub(super) enum PlaceStep<'expr, 'src> {
    Index {
        index: &'expr ast::Expr<'src>,
        /// What the index selects from.
        object: Span,
        /// The target up to and including this index.
        span: Span,
    },
    Field {
        field: ast::Name<'src>,
        /// What the field belongs to.
        object: Span,
    },
}

impl<'expr, 'src> PlaceStep<'expr, 'src> {
    /// The index this step computes, if it is one.
    pub fn index(&self) -> Option<&'expr ast::Expr<'src>> {
        match self {
            Self::Index { index, .. } => Some(index),
            Self::Field { .. } => None,
        }
    }
}

/**
 * Splits an assignment target into the expression it starts from and its
 * steps, from the outermost inward. `grid[row].cells[column]` starts from
 * `grid` and takes three steps.
 */
pub(super) fn place_parts<'expr, 'src>(
    target: &'expr ast::Expr<'src>,
) -> (&'expr ast::Expr<'src>, Vec<PlaceStep<'expr, 'src>>) {
    let mut steps = Vec::new();
    let mut root = target;
    loop {
        let (step, object) = match &root.kind {
            ast::ExprKind::Index { object, index } => (
                PlaceStep::Index {
                    index,
                    object: object.span,
                    span: root.span,
                },
                object,
            ),
            ast::ExprKind::Field { object, field } => (
                PlaceStep::Field {
                    field: *field,
                    object: object.span,
                },
                object,
            ),
            _ => break,
        };
        steps.push(step);
        root = object;
    }
    steps.reverse();
    (root, steps)
}

impl<'src> Checker<'src, '_> {
    /**
     * Checks the steps of an assignment target from the binding inward, and
     * returns them with the type of the part they reach. `ty` is the type of
     * the binding.
     */
    pub(super) fn check_place_steps(
        &mut self,
        mut ty: Type,
        steps: &[PlaceStep<'_, 'src>],
    ) -> CheckResult<(Vec<ir::PlaceStep>, Type)> {
        let mut checked = Vec::with_capacity(steps.len());
        for step in steps {
            match step {
                PlaceStep::Index {
                    index,
                    object,
                    span,
                } => {
                    let value = self.check_index_value(index)?;
                    let (element, length) = self.elements_of(&ty, *object)?;
                    checked.push(ir::PlaceStep::Index(ir::PlaceIndex {
                        checked: self.needs_bounds_check(&value, length)?,
                        value,
                        length,
                        span: *span,
                    }));
                    ty = element;
                }
                PlaceStep::Field { field, object } => {
                    let (position, field_type) = self.field_of(&ty, *field, *object)?;
                    checked.push(ir::PlaceStep::Field(position));
                    ty = field_type;
                }
            }
        }
        Ok((checked, ty))
    }
}
