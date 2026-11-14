/*!
 * Floats: their literals, arithmetic, negation and conversions (spec 4.1).
 *
 * A float operation follows IEEE 754 in the type of its operands, as the
 * program computes it on every system. A value known while compiling is
 * computed now the same way; one that would be infinity or NaN is an error,
 * since nothing that only exists at run time made it so.
 */

use super::errors;
use super::expressions::literal;
use super::values::{self, FloatLiteralError, FoldError};
use super::{CheckResult, Checker};
use crate::ast;
use crate::ir::{self, ArithmeticOperator, FloatType, IntegerType, Type, Value};
use crate::source::Span;

/// The type both operands of an arithmetic operator share, or the type one value is converted between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Number {
    Integer(IntegerType),
    Float(FloatType),
}

impl Number {
    pub fn of(ty: &Type) -> Option<Number> {
        match ty {
            Type::Integer(integer) => Some(Number::Integer(*integer)),
            Type::Float(float) => Some(Number::Float(*float)),
            Type::Boolean | Type::String | Type::Array(_) | Type::Custom(_) | Type::Choice(_) => {
                None
            }
        }
    }

    pub fn ty(self) -> Type {
        match self {
            Number::Integer(integer) => Type::Integer(integer),
            Number::Float(float) => Type::Float(float),
        }
    }
}

/// The float type a literal takes: the one `hint` asks for, else `float64`.
fn float_type(hint: Option<&Type>) -> FloatType {
    hint.and_then(Type::as_float).unwrap_or(FloatType::DEFAULT)
}

impl<'src> Checker<'src, '_> {
    /// An expression made only of float literals, typed by its context. `None` when `expr` is not one.
    pub(super) fn check_float_literal(
        &mut self,
        expr: &ast::Expr<'src>,
        hint: Option<&Type>,
    ) -> Option<CheckResult<ir::Expr>> {
        let ty = float_type(hint);
        let value = values::literal_float(expr, ty, &|span| self.text(span))?;
        Some(self.float_value(value, ty, expr.span))
    }

    /// One float literal, as written at `span`. `value` is its nearest `float64`.
    pub(super) fn check_float_number(
        &mut self,
        value: f64,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<ir::Expr> {
        let ty = float_type(hint);
        let value = values::float_literal_value(self.text(span), value, ty, span);
        self.float_value(value, ty, span)
    }

    fn float_value(
        &mut self,
        value: Result<f64, FloatLiteralError>,
        ty: FloatType,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        match value {
            Ok(value) => Ok(literal(Value::float(value, ty), span)),
            Err(FloatLiteralError::DivisionByZero(divisor)) => {
                Err(self.report(errors::division_by_zero(divisor)))
            }
            Err(FloatLiteralError::TooLarge(part)) => {
                Err(self.report(errors::float_too_large(ty, part)))
            }
            Err(FloatLiteralError::TooSmall(part)) => {
                Err(self.report(errors::float_too_small(ty, part)))
            }
        }
    }

    /// `left operator right` on two floats of `ty`. Floats have no `%`.
    pub(super) fn check_float_arithmetic(
        &mut self,
        operator: ArithmeticOperator,
        left: ir::Expr,
        right: ir::Expr,
        ty: FloatType,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        if operator == ArithmeticOperator::Remainder {
            return Err(self.report(errors::float_remainder(ty, span)));
        }
        if let (
            Some(Value::Float {
                bits: left_bits, ..
            }),
            Some(Value::Float {
                bits: right_bits, ..
            }),
        ) = (self.known_value(&left), self.known_value(&right))
        {
            let folded = values::fold_float(
                operator,
                f64::from_bits(left_bits),
                f64::from_bits(right_bits),
                ty,
            );
            return match folded {
                Ok(value) => Ok(literal(Value::float(value, ty), span)),
                Err(FoldError::DivisionByZero) => {
                    Err(self.report(errors::division_by_zero(right.span)))
                }
                Err(FoldError::Overflow) => Err(self.report(errors::float_too_large(ty, span))),
            };
        }
        Ok(ir::Expr {
            kind: ir::ExprKind::Arithmetic {
                operator,
                left: Box::new(left),
                right: Box::new(right),
                checked: false,
            },
            ty: Type::Float(ty),
            span,
        })
    }

    /// `-operand` for a float, which only flips the sign and so never fails.
    pub(super) fn negate_float(&self, operand: ir::Expr, ty: FloatType, span: Span) -> ir::Expr {
        if let Some(Value::Float { bits, .. }) = self.known_value(&operand) {
            return literal(Value::float(-f64::from_bits(bits), ty), span);
        }
        ir::Expr {
            kind: ir::ExprKind::Negate {
                operand: Box::new(operand),
                checked: false,
            },
            ty: Type::Float(ty),
            span,
        }
    }

    /**
     * A conversion that involves a float. An integer becomes the nearest
     * float. A float becomes an integer by dropping its fraction, and the
     * program stops if the result does not fit or the float is not a number;
     * a `float64` becomes the nearest `float32`, and the program stops if it
     * is beyond the largest one. A known value is converted now.
     */
    pub(super) fn convert_float(
        &mut self,
        operand: ir::Expr,
        source: Number,
        target: Number,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        let known = match self.known_value(&operand) {
            Some(Value::Integer { value, .. }) => Some(KnownNumber::Integer(value)),
            Some(Value::Float { bits, .. }) => Some(KnownNumber::Float(f64::from_bits(bits))),
            Some(Value::Boolean(_) | Value::Text(_)) | None => None,
        };
        if let Some(known) = known {
            let converted = known.convert(target).ok_or_else(|| {
                let shown = format!("`{}`", self.text(operand.span));
                errors::conversion_never_fits(&shown, &target.ty(), operand.span)
            });
            return match converted {
                Ok(value) => Ok(literal(value, span)),
                Err(error) => Err(self.report(error)),
            };
        }
        let checked = match (source, target) {
            (Number::Float(_), Number::Integer(_)) => true,
            (Number::Float(from), Number::Float(to)) => {
                from == FloatType::Float64 && to == FloatType::Float32
            }
            (Number::Integer(_), _) => false,
        };
        Ok(ir::Expr {
            kind: ir::ExprKind::Convert {
                operand: Box::new(operand),
                checked,
            },
            ty: target.ty(),
            span,
        })
    }
}

/// A number known while compiling, before it is converted.
#[derive(Clone, Copy)]
enum KnownNumber {
    Integer(i128),
    Float(f64),
}

impl KnownNumber {
    /// The value converted to `target`, or `None` when it does not fit.
    fn convert(self, target: Number) -> Option<Value> {
        match (self, target) {
            (KnownNumber::Integer(value), Number::Integer(ty)) => {
                ty.contains(value).then_some(Value::Integer { value, ty })
            }
            // One rounding, straight from the integer: through `float64` first could miss the nearest `float32`.
            (KnownNumber::Integer(value), Number::Float(FloatType::Float32)) => {
                Some(Value::float(f64::from(value as f32), FloatType::Float32))
            }
            (KnownNumber::Integer(value), Number::Float(FloatType::Float64)) => {
                Some(Value::float(value as f64, FloatType::Float64))
            }
            (KnownNumber::Float(value), Number::Integer(ty)) => {
                // A whole float within the range of `i128` converts to it exactly, so the range test is exact too.
                let whole = value.trunc();
                if !i128_holds(whole) {
                    return None;
                }
                let whole = whole as i128;
                ty.contains(whole)
                    .then_some(Value::Integer { value: whole, ty })
            }
            (KnownNumber::Float(value), Number::Float(ty)) => {
                let rounded = ty.round(value);
                rounded.is_finite().then(|| Value::float(rounded, ty))
            }
        }
    }
}

/// Whether a whole float lies within the range of `i128`, so a cast to it is exact. NaN does not.
fn i128_holds(whole: f64) -> bool {
    let limit = 2.0_f64.powi(127);
    whole >= -limit && whole < limit
}
