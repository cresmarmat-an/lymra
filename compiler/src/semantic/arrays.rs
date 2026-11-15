/*!
 * Arrays: their types, indexing, `array(value)`, `length`, and the elements
 * an assignment can change. Lists are indexed and measured the same way.
 *
 * An index known while compiling is checked while compiling against an
 * array's length (`L0411`), and against 0 for a list; any other index is
 * checked when the program runs, unless the optimizer proves it in range.
 */

use super::errors;
use super::expressions::CheckedCall;
use super::features::Feature;
use super::{CheckResult, Checker, Global};
use crate::ast::{self, TypeArgument};
use crate::diagnostics::ErrorReported;
use crate::ir::{self, Extent, IntegerType, Type, Value};
use crate::source::Span;

/// The most bytes one value on the call stack may take, an array or a value of a declared type. The stack holds a few MiB in all.
pub const MAX_VALUE_SIZE: u64 = 1 << 20;

impl<'src> Checker<'src, '_> {
    /// Resolves `array[element, length]`. The length is a number or a top-level integer constant.
    pub(super) fn resolve_array_type(
        &mut self,
        arguments: &[TypeArgument<'src>],
        span: Span,
    ) -> CheckResult<Type> {
        let [TypeArgument::Type(element), length] = arguments else {
            return Err(self.report(errors::malformed_array_type(span)));
        };
        let element = self.resolve_type(element);
        let (length, length_span) = match length {
            TypeArgument::Size { value, span } => (Ok(*value), *span),
            TypeArgument::Type(named) => (self.length_constant(named), named.span),
        };
        let (element_type, length) = (element?, length?);
        let nested = if element_type.is_growable() {
            Some(Feature::NestedGrowable)
        } else if element_type.holds_view() {
            Some(Feature::NestedView)
        } else {
            None
        };
        if let Some(feature) = nested {
            let element_span = match &arguments[0] {
                TypeArgument::Type(element) => element.span,
                TypeArgument::Size { span, .. } => *span,
            };
            return Err(self.report(feature.diagnostic(element_span)));
        }
        let element = element_type;
        if length == 0 {
            return Err(self.report(errors::empty_array(length_span)));
        }
        self.fits_on_stack(Type::array(element, length), span)
    }

    /// `ty` if a value of it fits on the call stack, else error `L0410`, reported at `span`.
    pub(super) fn fits_on_stack(&mut self, ty: Type, span: Span) -> CheckResult<Type> {
        match ty.size() {
            Some(size) if size <= MAX_VALUE_SIZE => Ok(ty),
            size => Err(self.report(errors::too_large_for_stack(&ty, size, span))),
        }
    }

    /// The value of a top-level constant named as an array's length.
    fn length_constant(&mut self, named: &ast::TypeExpr<'src>) -> CheckResult<u64> {
        let malformed =
            |checker: &mut Self| checker.report(errors::malformed_array_type(named.span));
        let ast::TypeExprKind::Named { path, arguments } = &named.kind else {
            return Err(malformed(self));
        };
        let ([name], true) = (path.segments.as_slice(), arguments.is_empty()) else {
            return Err(malformed(self));
        };
        let Some(Global::Constant(id)) = self.globals.get(name.text).copied() else {
            return Err(malformed(self));
        };
        if let Some(entry) = self.constants.get_mut(id) {
            entry.used = true;
        }
        match self.constant_value(id, Some(named.span))? {
            Value::Integer { value, .. } => u64::try_from(value)
                .map_err(|_| self.report(errors::negative_length(name.text, value, named.span))),
            Value::Float { .. } | Value::Boolean(_) | Value::Text(_) => Err(malformed(self)),
        }
    }

    /// Checks `items[index]`, on an array or a list.
    pub(super) fn check_index(
        &mut self,
        object: &ast::Expr<'src>,
        index: &ast::Expr<'src>,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        let array = self.check_expr_with_hint(object, None);
        let index = self.check_index_value(index);
        let (array, index) = (array?, index?);
        let (element, length) = self.elements_of(&array.ty, array.span)?;
        let checked = self.needs_bounds_check(&index, length)?;
        Ok(ir::Expr {
            kind: ir::ExprKind::Index {
                array: Box::new(array),
                index: Box::new(index),
                length,
                checked,
            },
            ty: element,
            span,
        })
    }

    /// The element type and the length of a value that can be indexed: an array or a list.
    pub(super) fn elements_of(&mut self, ty: &Type, span: Span) -> CheckResult<(Type, Extent)> {
        match ty {
            Type::Array(array) => Ok((array.element.clone(), Extent::Fixed(array.length))),
            Type::List(list) => Ok((list.element.clone(), Extent::Dynamic)),
            Type::View(view) => Ok((view.element.clone(), Extent::Dynamic)),
            other => Err(self.report(errors::not_indexable(other, span))),
        }
    }

    /// Checks an index, which can have any integer type. A literal is an `integer64`.
    pub(super) fn check_index_value(&mut self, index: &ast::Expr<'src>) -> CheckResult<ir::Expr> {
        let index = self.check_expr_with_hint(index, None)?;
        if index.ty.as_integer().is_none() {
            return Err(self.report(errors::index_not_integer(&index.ty, index.span)));
        }
        Ok(index)
    }

    /**
     * Whether `index` has to be checked when the program runs. One known now
     * is checked now: against an array's length, and against 0 for a list,
     * whose length only the running program knows.
     */
    pub(super) fn needs_bounds_check(
        &mut self,
        index: &ir::Expr,
        length: Extent,
    ) -> CheckResult<bool> {
        let Some(Value::Integer { value, .. }) = self.known_value(index) else {
            return Ok(true);
        };
        match length {
            Extent::Fixed(length) if (0..i128::from(length)).contains(&value) => Ok(false),
            Extent::Fixed(length) => {
                Err(self.report(errors::index_out_of_range(value, length, index.span)))
            }
            Extent::Dynamic if value < 0 => {
                Err(self.report(errors::negative_index(value, index.span)))
            }
            Extent::Dynamic => Ok(true),
        }
    }

    /// Checks `array(value)`, which builds the array type `hint` with every element set to `value`.
    pub(super) fn check_fill(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<CheckedCall<'src>> {
        let [argument] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::fill_argument_count(call.arguments.len(), arguments)));
        };
        let value = self.single_value_argument(argument, "value")?;
        let Some(ty @ Type::Array(array)) = hint else {
            self.check_for_errors(value);
            return Err(match hint {
                Some(expected) => self.report(errors::fill_into(expected, span)),
                None if self.destination_unknown => ErrorReported,
                None => self.report(errors::fill_without_type(span)),
            });
        };
        let value = self.check_expr(value, Some(&array.element))?;
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Fill(Box::new(value)),
            ty: ty.clone(),
            span,
        }))
    }

    /**
     * Checks `length(value)`. An array's length is part of its type, so it
     * is known now unless computing the array has effects; a text's length
     * is counted when the program runs.
     */
    pub(super) fn check_length(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let [argument] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::length_argument_count(
                call.arguments.len(),
                arguments,
            )));
        };
        let value = self.single_value_argument(argument, "items")?;
        let operand = self.check_expr_with_hint(value, None)?;
        let known = match &operand.ty {
            Type::Array(array) => (!operand.has_effects()).then_some(array.length),
            Type::String | Type::List(_) | Type::View(_) => None,
            Type::Integer(_)
            | Type::Float(_)
            | Type::Boolean
            | Type::Custom(_)
            | Type::Choice(_) => {
                return Err(self.report(errors::no_length(&operand.ty, operand.span)));
            }
        };
        let kind = match known {
            Some(length) => ir::ExprKind::Literal(Value::Integer {
                value: i128::from(length),
                ty: IntegerType::Integer64,
            }),
            None => ir::ExprKind::Length(Box::new(operand)),
        };
        Ok(CheckedCall::Value(ir::Expr {
            kind,
            ty: Type::Integer(IntegerType::Integer64),
            span,
        }))
    }
}
