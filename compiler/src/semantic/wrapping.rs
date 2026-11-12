//! `wrapping_add`, `wrapping_subtract` and `wrapping_multiply`: arithmetic that wraps around its type instead of stopping the program.

use super::expressions::CheckedCall;
use super::{CheckResult, Checker, errors, values};
use crate::ast;
use crate::ir::{self, Type, Value, WrappingOperator};
use crate::source::Span;

impl<'src> Checker<'src, '_> {
    /**
     * Checks a call such as `wrapping_add(left, right)`. The operands follow
     * the rules of `+`: two integers of one type, where a literal takes the
     * other's type. When both are literals, they take the type `hint` asks
     * for, so `constant unsigned8 total = wrapping_add(200, 100)` holds 44.
     */
    pub(super) fn check_wrapping(
        &mut self,
        operator: WrappingOperator,
        call: &ast::Call<'src>,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<CheckedCall<'src>> {
        let name = operator.function();
        let [left, right] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::wrapping_argument_count(
                name,
                call.arguments.len(),
                arguments,
            )));
        };
        let left = self.single_value_argument(left, "left");
        let right = self.single_value_argument(right, "right");
        let (left, right) = (left?, right?);
        let both_literals =
            values::literal_integer(left).is_some() && values::literal_integer(right).is_some();
        let (left, right) = match hint {
            Some(ty @ Type::Integer(_)) if both_literals => {
                let left = self.check_expr_with_hint(left, Some(ty));
                let right = self.check_expr_with_hint(right, Some(ty));
                (left?, right?)
            }
            _ => self.check_operands(left, right)?,
        };
        let ty = self.shared_integer_type(name, &left, &right)?;
        if let (
            Some(Value::Integer { value: left, .. }),
            Some(Value::Integer { value: right, .. }),
        ) = (self.known_value(&left), self.known_value(&right))
        {
            let value = operator.apply(left, right, ty);
            return Ok(CheckedCall::Value(ir::Expr {
                kind: ir::ExprKind::Literal(Value::Integer { value, ty }),
                ty: Type::Integer(ty),
                span,
            }));
        }
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Wrapping {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            },
            ty: Type::Integer(ty),
            span,
        }))
    }
}
