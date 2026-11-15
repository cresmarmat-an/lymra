/*!
 * Lists and stacks: their types, `list()`, `stack()`, `push`, `pop` and
 * `top` (spec 4.4). A list grows in place on a data stack of its own, so its
 * elements cannot hold another list until values hold growable parts. A
 * stack is a list that can also give back its top element.
 */

use super::errors;
use super::expressions::CheckedCall;
use super::features::Feature;
use super::{CheckResult, Checker};
use crate::ast::{self, ArgumentKind, TypeArgument};
use crate::diagnostics::ErrorReported;
use crate::ir::{self, Sequence, Type};
use crate::source::Span;

impl<'src> Checker<'src, '_> {
    /// Resolves `list[element]` or `stack[element]`, as `kind` says.
    pub(super) fn resolve_list_type(
        &mut self,
        arguments: &[TypeArgument<'src>],
        span: Span,
        kind: Sequence,
    ) -> CheckResult<Type> {
        let [TypeArgument::Type(element)] = arguments else {
            return Err(self.report(errors::malformed_list_type(kind, span)));
        };
        let element_type = self.resolve_type(element)?;
        if element_type.is_growable() {
            return Err(self.report(Feature::NestedGrowable.diagnostic(element.span)));
        }
        if element_type.holds_view() {
            return Err(self.report(Feature::NestedView.diagnostic(element.span)));
        }
        Ok(Type::sequence(element_type, kind))
    }

    /// Checks `list()` or `stack()`, as `kind` says: one with no elements, of the type `hint`.
    pub(super) fn check_empty_list(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
        hint: Option<&Type>,
        kind: Sequence,
    ) -> CheckResult<CheckedCall<'src>> {
        if !call.arguments.is_empty() {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::empty_list_arguments(
                kind,
                call.arguments.len(),
                arguments,
            )));
        }
        match hint {
            Some(ty @ Type::List(list)) if list.kind == kind => Ok(CheckedCall::Value(ir::Expr {
                kind: ir::ExprKind::EmptyList,
                ty: ty.clone(),
                span,
            })),
            Some(expected) => Err(self.report(errors::empty_list_into(kind, expected, span))),
            None if self.destination_unknown => Err(ErrorReported),
            None => Err(self.report(errors::empty_list_without_type(kind, span))),
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

    /// Checks `pop(variable stack)`, which takes the top element off a stack and gives it.
    pub(super) fn check_pop(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let [argument] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::stack_argument_count(
                "pop",
                call.arguments.len(),
                arguments,
            )));
        };
        let place = match &argument.kind {
            ArgumentKind::Mutable(place) => place,
            ArgumentKind::Positional(value) => {
                self.check_for_errors(value);
                if let ast::ExprKind::Name(name) = &value.kind {
                    self.mark_name_changed(name);
                }
                let source = self.text(value.span);
                return Err(self.report(errors::pop_needs_variable(value.span, source)));
            }
            ArgumentKind::Named { value, .. } => {
                self.mark_reads(value);
                return Err(self.report(Feature::NamedArguments.diagnostic(argument.span)));
            }
        };
        let (id, ty) = self.check_reference_place(place)?;
        let element = self.stack_element("pop", &ty, place.span)?;
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Pop(id),
            ty: element,
            span,
        }))
    }

    /// Checks `top(stack)`, the top element of a stack, which stays on it.
    pub(super) fn check_top(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let [argument] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::stack_argument_count(
                "top",
                call.arguments.len(),
                arguments,
            )));
        };
        let value = self.single_value_argument(argument, "items")?;
        let stack = self.check_expr_with_hint(value, None)?;
        let element = self.stack_element("top", &stack.ty, stack.span)?;
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Top(Box::new(stack)),
            ty: element,
            span,
        }))
    }

    /// The type of the elements of `ty`, a stack that `function` takes from, or else the error at `span`.
    fn stack_element(&mut self, function: &str, ty: &Type, span: Span) -> CheckResult<Type> {
        match ty {
            Type::List(list) if list.kind == Sequence::Stack => Ok(list.element.clone()),
            other => Err(self.report(errors::not_a_stack(function, other, span))),
        }
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
