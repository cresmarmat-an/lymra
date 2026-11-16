/*!
 * `bit_and`, `bit_or`, `bit_xor` and `bit_not`, which work on the bits of an
 * integer, and `shift_left` and `shift_right`, which move them by a count.
 * None of them can overflow; a shift stops the program when its count falls
 * outside the bits of the value, which is an error when the count is known
 * while compiling (L0416).
 */

use super::expressions::CheckedCall;
use super::{CheckResult, Checker, errors, values};
use crate::ast;
use crate::ir::{self, BitOperator, IntegerType, Type, Value};
use crate::source::Span;

impl<'src> Checker<'src, '_> {
    /**
     * A call to `bit_and`, `bit_or` or `bit_xor`, whose operands follow the
     * rules of `+`, or to `shift_left` or `shift_right`, whose count can have
     * any integer type. When everything is known, the result is too.
     */
    pub(super) fn check_bits(
        &mut self,
        operator: BitOperator,
        call: &ast::Call<'src>,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<CheckedCall<'src>> {
        let name = operator.function();
        let parameters: &[&str] = if operator.is_shift() {
            &["value", "count"]
        } else {
            &["left", "right"]
        };
        let [left, right] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::bits_argument_count(
                name,
                parameters,
                call.arguments.len(),
                arguments,
            )));
        };
        let left = self.single_value_argument(left, parameters[0]);
        let right = self.single_value_argument(right, parameters[1]);
        let (left, right) = (left?, right?);
        let (left, right) = if operator.is_shift() {
            let value = self.check_hinted(left, hint);
            let count = self.check_expr_with_hint(right, None);
            (value?, count?)
        } else if values::is_literal(left) && values::is_literal(right) {
            let left = self.check_hinted(left, hint);
            let right = self.check_hinted(right, hint);
            (left?, right?)
        } else {
            self.check_operands(left, right)?
        };
        let left_type = self.bit_operand(name, &left);
        let right_type = self.bit_operand(name, &right);
        let (ty, count_type) = (left_type?, right_type?);
        if !operator.is_shift() && ty != count_type {
            return Err(self.report(errors::operand_mismatch(
                name,
                (&left.ty, left.span),
                (&right.ty, right.span),
            )));
        }
        let known_right = self.known_integer(&right);
        if let Some(count) = known_right.filter(|_| operator.is_shift()) {
            if operator.apply(0, count, ty).is_none() {
                return Err(self.report(errors::shift_count_out_of_range(
                    name, count, ty, right.span,
                )));
            }
        }
        Ok(CheckedCall::Value(self.bits_value(
            operator,
            left,
            right,
            ty,
            known_right.is_none(),
            span,
        )))
    }

    /// `bit_not(value)`: the value with every bit flipped, which is `bit_xor` with every bit set.
    pub(super) fn check_bit_not(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<CheckedCall<'src>> {
        let name = "bit_not";
        let [value] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::bits_argument_count(
                name,
                &["value"],
                call.arguments.len(),
                arguments,
            )));
        };
        let value = self.single_value_argument(value, "value")?;
        let value = self.check_hinted(value, hint)?;
        let ty = self.bit_operand(name, &value)?;
        let every_bit = ir::Expr {
            kind: ir::ExprKind::Literal(Value::Integer {
                value: ty.from_bits(u64::MAX),
                ty,
            }),
            ty: Type::Integer(ty),
            span: value.span,
        };
        Ok(CheckedCall::Value(self.bits_value(
            BitOperator::Xor,
            value,
            every_bit,
            ty,
            false,
            span,
        )))
    }

    /// An operand that takes the integer type `hint` asks for when it is a number written out, as in `bit_not(0)`.
    fn check_hinted(
        &mut self,
        operand: &ast::Expr<'src>,
        hint: Option<&Type>,
    ) -> CheckResult<ir::Expr> {
        let hint = hint.filter(|ty| matches!(ty, Type::Integer(_)));
        self.check_expr_with_hint(operand, hint)
    }

    /// The integer type of an operand of the bit function `function`, or an error for any other type.
    fn bit_operand(&mut self, function: &str, operand: &ir::Expr) -> CheckResult<IntegerType> {
        match operand.ty {
            Type::Integer(ty) => Ok(ty),
            _ => Err(self.report(errors::bit_operand_type(
                function,
                &operand.ty,
                operand.span,
            ))),
        }
    }

    fn known_integer(&self, expr: &ir::Expr) -> Option<i128> {
        match self.known_value(expr)? {
            Value::Integer { value, .. } => Some(value),
            Value::Float { .. } | Value::Boolean(_) | Value::Text(_) => None,
        }
    }

    /**
     * The operation on `left`, of type `ty`, and `right`, or its value when
     * both are known: a count known while compiling was checked already, so
     * only an unknown one leaves the shift `checked`.
     */
    fn bits_value(
        &self,
        operator: BitOperator,
        left: ir::Expr,
        right: ir::Expr,
        ty: IntegerType,
        checked: bool,
        span: Span,
    ) -> ir::Expr {
        let known = self
            .known_integer(&left)
            .zip(self.known_integer(&right))
            .and_then(|(left, right)| operator.apply(left, right, ty));
        let kind = match known {
            Some(value) => ir::ExprKind::Literal(Value::Integer { value, ty }),
            None => ir::ExprKind::Bits {
                operator,
                left: Box::new(left),
                right: Box::new(right),
                checked: checked && operator.is_shift(),
            },
        };
        ir::Expr {
            kind,
            ty: Type::Integer(ty),
            span,
        }
    }
}
