/*!
 * Views: their types, `slice`, `bytes`, and the view an array or a list
 * becomes where a view of its elements is expected (spec 4.4). A view reads
 * elements that something else keeps, so it never holds a list and nothing
 * holds a view: a value that holds one would need to know what it shows.
 */

use super::errors;
use super::expressions::CheckedCall;
use super::features::Feature;
use super::{CheckResult, Checker};
use crate::ast::{self, TypeArgument};
use crate::ir::{self, IntegerType, Type, Value};
use crate::source::Span;

/**
 * `value` as a value of `expected`. An array or a list where a view of its
 * elements is expected becomes that view, with nothing copied; anything
 * else stays as it is, for the caller to compare with `expected`.
 */
pub(super) fn seen_as(value: ir::Expr, expected: &Type) -> ir::Expr {
    let Some(view) = expected.as_view() else {
        return value;
    };
    let element = match (value.ty.as_array(), value.ty.as_list()) {
        (Some(array), _) => &array.element,
        (None, Some(list)) => &list.element,
        (None, None) => return value,
    };
    if *element != view.element {
        return value;
    }
    ir::Expr {
        span: value.span,
        ty: expected.clone(),
        kind: ir::ExprKind::View(Box::new(value)),
    }
}

/// The type a value needs where `expected` is wanted: a list for a view, so `list()` knows what to build.
pub(super) fn hint_for(expected: &Type) -> Option<Type> {
    expected
        .as_view()
        .map(|view| Type::list(view.element.clone()))
}

impl<'src> Checker<'src, '_> {
    /// Resolves `view[element]`.
    pub(super) fn resolve_view_type(
        &mut self,
        arguments: &[TypeArgument<'src>],
        span: Span,
    ) -> CheckResult<Type> {
        let [TypeArgument::Type(element)] = arguments else {
            return Err(self.report(errors::malformed_view_type(span)));
        };
        let element_type = self.resolve_type(element)?;
        if element_type.is_growable() {
            return Err(self.report(Feature::NestedGrowable.diagnostic(element.span)));
        }
        if element_type.holds_view() {
            return Err(self.report(Feature::NestedView.diagnostic(element.span)));
        }
        Ok(Type::view(element_type))
    }

    /**
     * Checks `slice(items, start, end)`: the elements of an array, a list or
     * a view from `start` up to `end`, as a view, or the bytes of a text
     * between them, as a text. Positions known now are checked now.
     */
    pub(super) fn check_slice(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let [items, start, end] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::slice_argument_count(
                call.arguments.len(),
                arguments,
            )));
        };
        let items = self
            .single_value_argument(items, "items")
            .and_then(|items| self.check_expr_with_hint(items, None));
        let start = self
            .single_value_argument(start, "start")
            .and_then(|start| self.check_position(start));
        let end = self
            .single_value_argument(end, "end")
            .and_then(|end| self.check_position(end));
        let (items, start, end) = (items?, start?, end?);
        let (ty, length) = match &items.ty {
            Type::String => (Type::String, None),
            Type::Array(array) => (Type::view(array.element.clone()), Some(array.length)),
            Type::List(list) => (Type::view(list.element.clone()), None),
            Type::View(_) => (items.ty.clone(), None),
            Type::Integer(_)
            | Type::Float(_)
            | Type::Boolean
            | Type::Custom(_)
            | Type::Choice(_) => {
                return Err(self.report(errors::not_sliceable(&items.ty, items.span)));
            }
        };
        self.check_known_positions(&start, &end, length)?;
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Slice {
                items: Box::new(items),
                start: Box::new(start),
                end: Box::new(end),
            },
            ty,
            span,
        }))
    }

    /// A position of a slice, which can be any integer.
    fn check_position(&mut self, position: &ast::Expr<'src>) -> CheckResult<ir::Expr> {
        let position = self.check_expr_with_hint(position, None)?;
        if position.ty.as_integer().is_none() {
            return Err(self.report(errors::position_not_integer(&position.ty, position.span)));
        }
        Ok(position)
    }

    /**
     * Reports the ends of a slice that are known now and could never make
     * one: below 0, past the end of an array of `length` elements, or an end
     * before the start.
     */
    fn check_known_positions(
        &mut self,
        start: &ir::Expr,
        end: &ir::Expr,
        length: Option<u64>,
    ) -> CheckResult<()> {
        let known = |checker: &Self, position: &ir::Expr| match checker.known_value(position) {
            Some(Value::Integer { value, .. }) => Some(value),
            _ => None,
        };
        let (start_value, end_value) = (known(self, start), known(self, end));
        for (part, position, value) in [("start", start, start_value), ("end", end, end_value)] {
            let Some(value) = value else {
                continue;
            };
            let outside = value < 0 || length.is_some_and(|length| value > i128::from(length));
            if outside {
                return Err(self.report(errors::slice_position_out_of_range(
                    part,
                    value,
                    length,
                    position.span,
                )));
            }
        }
        if let (Some(start_value), Some(end_value)) = (start_value, end_value) {
            if end_value < start_value {
                return Err(self.report(errors::slice_backwards(start_value, end_value, end.span)));
            }
        }
        Ok(())
    }

    /// Checks `bytes(text)`: the bytes of a text, as a `view[unsigned8]`.
    pub(super) fn check_bytes(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let [argument] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::bytes_argument_count(
                call.arguments.len(),
                arguments,
            )));
        };
        let value = self.single_value_argument(argument, "text")?;
        let text = self.check_expr_with_hint(value, None)?;
        if text.ty != Type::String {
            return Err(self.report(errors::bytes_of_non_text(&text.ty, text.span)));
        }
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Bytes(Box::new(text)),
            ty: Type::view(Type::Integer(IntegerType::Unsigned8)),
            span,
        }))
    }
}
