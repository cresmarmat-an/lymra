/*!
 * Lists: their types, `list()` and `push` (spec 4.4). A list grows in place
 * on a data stack of its own, so its elements cannot hold another list until
 * values hold growable parts.
 */

use super::errors;
use super::expressions::CheckedCall;
use super::features::Feature;
use super::{CheckResult, Checker};
use crate::ast::{self, ArgumentKind, TypeArgument};
use crate::diagnostics::ErrorReported;
use crate::ir::{self, Type};
use crate::source::Span;

impl<'src> Checker<'src, '_> {
    /// Resolves `list[element]`.
    pub(super) fn resolve_list_type(
        &mut self,
        arguments: &[TypeArgument<'src>],
        span: Span,
    ) -> CheckResult<Type> {
        let [TypeArgument::Type(element)] = arguments else {
            return Err(self.report(errors::malformed_list_type(span)));
        };
        let element_type = self.resolve_type(element)?;
        if element_type.is_growable() {
            return Err(self.report(Feature::NestedGrowable.diagnostic(element.span)));
        }
        if element_type.holds_view() {
            return Err(self.report(Feature::NestedView.diagnostic(element.span)));
        }
        Ok(Type::list(element_type))
    }

    /// Checks `list()`, a list with no elements, of the list type `hint`.
    pub(super) fn check_empty_list(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<CheckedCall<'src>> {
        if !call.arguments.is_empty() {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::empty_list_arguments(
                call.arguments.len(),
                arguments,
            )));
        }
        match hint {
            Some(ty @ Type::List(_)) => Ok(CheckedCall::Value(ir::Expr {
                kind: ir::ExprKind::EmptyList,
                ty: ty.clone(),
                span,
            })),
            Some(expected) => Err(self.report(errors::empty_list_into(expected, span))),
            None if self.destination_unknown => Err(ErrorReported),
            None => Err(self.report(errors::empty_list_without_type(span))),
        }
    }

    /// Checks `push(variable list, value)`, which computes the value and then adds it to the end of the list.
    pub(super) fn check_push(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let [target, value] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::push_argument_count(call.arguments.len(), arguments)));
        };
        let list = self.check_push_target(target);
        let element = match &list {
            Ok((_, element)) => Ok(element.clone()),
            Err(error) => Err(*error),
        };
        let value = match &value.kind {
            ArgumentKind::Positional(value) => self.check_against(value, Some(element.as_ref())),
            ArgumentKind::Mutable(place) => {
                self.check_for_errors(place);
                let mark = Span::new(value.span.start, place.span.start);
                Err(self.report(errors::pushed_value_marked(mark)))
            }
            ArgumentKind::Named { value: named, .. } => {
                self.mark_reads(named);
                Err(self.report(Feature::NamedArguments.diagnostic(value.span)))
            }
        };
        let ((list, _), value) = (list?, value?);
        Ok(CheckedCall::Effect {
            statement: ir::Statement::Push { list, value, span },
            function: "push",
        })
    }

    /// The list binding `push` adds to, which it changes, and the type of its elements.
    fn check_push_target(
        &mut self,
        target: &ast::Argument<'src>,
    ) -> CheckResult<(ir::LocalId, Type)> {
        match &target.kind {
            ArgumentKind::Mutable(place) => {
                let (id, ty) = self.check_reference_place(place)?;
                match ty {
                    Type::List(list) => Ok((id, list.element.clone())),
                    other => Err(self.report(errors::push_not_list(&other, place.span))),
                }
            }
            ArgumentKind::Positional(value) => {
                self.check_for_errors(value);
                if let ast::ExprKind::Name(name) = &value.kind {
                    self.mark_name_changed(name);
                }
                let source = self.text(value.span);
                Err(self.report(errors::push_needs_variable(value.span, source)))
            }
            ArgumentKind::Named { value, .. } => {
                self.mark_reads(value);
                Err(self.report(Feature::NamedArguments.diagnostic(target.span)))
            }
        }
    }
}
