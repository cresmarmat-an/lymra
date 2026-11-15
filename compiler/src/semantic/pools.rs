/*!
 * Pools and handles: their types, `pool()`, `insert`, `remove` and
 * `pool[handle]` (spec 7.6). A pool keeps values of one fixed size in slots,
 * so a slot that `remove` frees fits the next value `insert` stores. Which
 * pool a handle belongs to is worked out by the roots pass.
 */

use super::errors::{self, Collection, PoolChange};
use super::expressions::CheckedCall;
use super::features::Feature;
use super::{CheckResult, Checker, Global};
use crate::ast::{self, ArgumentKind, TypeArgument};
use crate::diagnostics::ErrorReported;
use crate::ir::{self, Type};
use crate::source::Span;

impl<'src> Checker<'src, '_> {
    /// Resolves `pool[element]`, whose element has to have one fixed size (`L0706`).
    pub(super) fn resolve_pool_type(
        &mut self,
        arguments: &[TypeArgument<'src>],
        span: Span,
    ) -> CheckResult<Type> {
        let [TypeArgument::Type(element)] = arguments else {
            return Err(self.report(errors::malformed_list_type(Collection::Pool, span)));
        };
        let element_type = self.resolve_type(element)?;
        self.check_pool_element(&element_type, element.span)?;
        Ok(Type::pool(element_type))
    }

    /**
     * Resolves `handle[element]`. A declared type is named without being
     * resolved, so a type can hold handles to values of its own type; its
     * size is checked where a pool of it is named.
     */
    pub(super) fn resolve_handle_type(
        &mut self,
        arguments: &[TypeArgument<'src>],
        span: Span,
    ) -> CheckResult<Type> {
        let [TypeArgument::Type(element)] = arguments else {
            return Err(self.report(errors::malformed_handle_type(span)));
        };
        if let ast::TypeExprKind::Named { path, arguments } = &element.kind {
            if let ([name], []) = (path.segments.as_slice(), arguments.as_slice()) {
                if let Some(Global::Type(_)) = self.globals.get(name.text) {
                    return Ok(Type::handle_to_declared(name.text));
                }
            }
        }
        let element_type = self.resolve_type(element)?;
        self.check_pool_element(&element_type, element.span)?;
        Ok(Type::handle(element_type))
    }

    /// Checks that a pool can hold values of `element`, written at `span`.
    fn check_pool_element(&mut self, element: &Type, span: Span) -> CheckResult<()> {
        if element.holds_view() {
            return Err(self.report(Feature::NestedView.diagnostic(span)));
        }
        if element.is_growable() || element.holds_text() {
            return Err(self.report(errors::pool_element_not_fixed(element, span)));
        }
        Ok(())
    }

    /// Checks `pool()`, a pool with no values, of the pool type `hint`.
    pub(super) fn check_empty_pool(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<CheckedCall<'src>> {
        if !call.arguments.is_empty() {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::empty_list_arguments(
                Collection::Pool,
                call.arguments.len(),
                arguments,
            )));
        }
        match hint {
            Some(ty @ Type::Pool(_)) => Ok(CheckedCall::Value(ir::Expr {
                kind: ir::ExprKind::EmptyList,
                ty: ty.clone(),
                span,
            })),
            Some(expected) => {
                Err(self.report(errors::empty_list_into(Collection::Pool, expected, span)))
            }
            None if self.destination_unknown => Err(ErrorReported),
            None => Err(self.report(errors::empty_list_without_type(Collection::Pool, span))),
        }
    }

    /// Checks `insert(variable pool, value)`, which stores the value and gives its handle.
    pub(super) fn check_insert(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let (pool, value) = self.pool_change(PoolChange::Insert, call, span)?;
        let element = match &pool {
            Ok((_, pool_type)) => pool_type
                .as_pool()
                .map(|pool_type| pool_type.element.clone())
                .ok_or(ErrorReported),
            Err(error) => Err(*error),
        };
        let value = self.check_against(value, Some(element.as_ref()));
        let ((pool, _), element, value) = (pool?, element?, value?);
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Insert {
                pool,
                value: Box::new(value),
            },
            ty: Type::handle(element),
            span,
        }))
    }

    /// Checks `remove(variable pool, handle)`, which frees the slot the handle names.
    pub(super) fn check_remove(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let (pool, handle) = self.pool_change(PoolChange::Remove, call, span)?;
        let handle = match &pool {
            Ok((_, pool_type)) => self.check_handle(handle, pool_type),
            Err(error) => {
                self.check_for_errors(handle);
                Err(*error)
            }
        };
        let ((pool, _), handle) = (pool?, handle?);
        Ok(CheckedCall::Effect {
            statement: ir::Statement::Remove { pool, handle, span },
            function: "remove",
        })
    }

    /**
     * The two arguments of `insert` or `remove`: the pool, passed with
     * `variable`, as its binding and its type, and the other argument,
     * which the call only reads.
     */
    fn pool_change<'call>(
        &mut self,
        change: PoolChange,
        call: &'call ast::Call<'src>,
        span: Span,
    ) -> CheckResult<(CheckResult<(ir::LocalId, Type)>, &'call ast::Expr<'src>)> {
        let [target, other] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::pool_argument_count(
                change,
                call.arguments.len(),
                arguments,
            )));
        };
        let pool = self.pool_target(change, target);
        let other = match &other.kind {
            ArgumentKind::Positional(value) => value,
            ArgumentKind::Mutable(place) => {
                self.check_for_errors(place);
                let mark = Span::new(other.span.start, place.span.start);
                return Err(self.report(errors::pool_argument_marked(change, mark)));
            }
            ArgumentKind::Named { value, .. } => {
                self.mark_reads(value);
                return Err(self.report(Feature::NamedArguments.diagnostic(other.span)));
            }
        };
        Ok((pool, other))
    }

    /// The pool `insert` or `remove` changes, passed with `variable`, and its type.
    fn pool_target(
        &mut self,
        change: PoolChange,
        target: &ast::Argument<'src>,
    ) -> CheckResult<(ir::LocalId, Type)> {
        match &target.kind {
            ArgumentKind::Mutable(place) => {
                let (id, ty) = self.check_reference_place(place)?;
                if ty.as_pool().is_none() {
                    return Err(self.report(errors::not_a_pool(change, &ty, place.span)));
                }
                Ok((id, ty))
            }
            ArgumentKind::Positional(value) => {
                self.check_for_errors(value);
                if let ast::ExprKind::Name(name) = &value.kind {
                    self.mark_name_changed(name);
                }
                let source = self.text(value.span);
                Err(self.report(errors::pool_needs_variable(change, value.span, source)))
            }
            ArgumentKind::Named { value, .. } => {
                self.mark_reads(value);
                Err(self.report(Feature::NamedArguments.diagnostic(target.span)))
            }
        }
    }

    /// Checks `pool[handle]` once `pool` is known to be a pool: the value in the slot the handle names.
    pub(super) fn check_slot(
        &mut self,
        pool: ir::Expr,
        handle: &ast::Expr<'src>,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        let handle = self.check_handle(handle, &pool.ty)?;
        let element = match pool.ty.as_pool() {
            Some(pool_type) => pool_type.element.clone(),
            None => return Err(ErrorReported),
        };
        Ok(ir::Expr {
            kind: ir::ExprKind::Slot {
                pool: Box::new(pool),
                handle: Box::new(handle),
            },
            ty: element,
            span,
        })
    }

    /// Checks a handle into a pool of type `pool`: one to the values that pool holds.
    pub(super) fn check_handle(
        &mut self,
        handle: &ast::Expr<'src>,
        pool: &Type,
    ) -> CheckResult<ir::Expr> {
        let Some(pool_type) = pool.as_pool() else {
            self.check_for_errors(handle);
            return Err(ErrorReported);
        };
        let expected = Type::handle(pool_type.element.clone());
        let checked = self.check_expr_with_hint(handle, Some(&expected))?;
        if checked.ty != expected {
            return Err(self.report(errors::slot_not_handle(pool, &checked.ty, checked.span)));
        }
        Ok(checked)
    }
}
