/*!
 * Compile-time arithmetic.
 *
 * Integer literals are computed exactly before they get a type, as in
 * `200 + 100`, so only the final value has to fit the type the context asks
 * for. Values of a known type, such as top-level constants, are computed with
 * that type's range, so an expression that would always overflow is caught
 * here instead of at run time.
 *
 * Floats are computed as the program computes them: each operation in the
 * type of its operands, rounded after each step, so a value known while
 * compiling is the value the program would have. One that is not a finite
 * number would be infinity or NaN, and is an error.
 */

use crate::ast::{self, BinaryOperator, UnaryOperator};
use crate::ir::{ArithmeticOperator, ComparisonOperator, FloatType, IntegerType, Value};
use crate::source::Span;

/// Why computing a value of a known type failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FoldError {
    Overflow,
    DivisionByZero,
}

/// Why computing an expression made only of literals failed, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LiteralError {
    /// The span of the divisor that is zero.
    DivisionByZero(Span),
    /// The span of the part whose value is too large to compute with.
    TooLarge(Span),
}

/// Why computing an expression made only of float literals failed, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FloatLiteralError {
    /// The span of the divisor that is zero.
    DivisionByZero(Span),
    /// The span of the part whose value is beyond the largest float of the type.
    TooLarge(Span),
    /// The span of a literal that is not zero but rounds to zero in the type.
    TooSmall(Span),
}

pub(super) fn arithmetic_operator(operator: BinaryOperator) -> Option<ArithmeticOperator> {
    match operator {
        BinaryOperator::Add => Some(ArithmeticOperator::Add),
        BinaryOperator::Subtract => Some(ArithmeticOperator::Subtract),
        BinaryOperator::Multiply => Some(ArithmeticOperator::Multiply),
        BinaryOperator::Divide => Some(ArithmeticOperator::Divide),
        BinaryOperator::Remainder => Some(ArithmeticOperator::Remainder),
        _ => None,
    }
}

/**
 * The exact value of an expression made only of integer literals, `-` and
 * arithmetic. Returns `None` for any other expression, including one that
 * mixes literals with names.
 */
pub(super) fn literal_integer(expr: &ast::Expr<'_>) -> Option<Result<i128, LiteralError>> {
    match &expr.kind {
        ast::ExprKind::Integer(value) => Some(Ok(i128::from(*value))),
        ast::ExprKind::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => {
            let operand = literal_integer(operand)?;
            Some(
                operand
                    .and_then(|value| value.checked_neg().ok_or(LiteralError::TooLarge(expr.span))),
            )
        }
        ast::ExprKind::Binary {
            operator,
            left,
            right,
        } => {
            let operator = arithmetic_operator(*operator)?;
            let left_value = literal_integer(left)?;
            let right_value = literal_integer(right)?;
            Some(left_value.and_then(|left_value| {
                let right_value = right_value?;
                compute(operator, left_value, right_value).map_err(|error| match error {
                    FoldError::DivisionByZero => LiteralError::DivisionByZero(right.span),
                    FoldError::Overflow => LiteralError::TooLarge(expr.span),
                })
            }))
        }
        _ => None,
    }
}

/// Exact arithmetic within the range of `i128`, with C's rounding: division truncates toward zero.
fn compute(operator: ArithmeticOperator, left: i128, right: i128) -> Result<i128, FoldError> {
    if right == 0
        && matches!(
            operator,
            ArithmeticOperator::Divide | ArithmeticOperator::Remainder
        )
    {
        return Err(FoldError::DivisionByZero);
    }
    let result = match operator {
        ArithmeticOperator::Add => left.checked_add(right),
        ArithmeticOperator::Subtract => left.checked_sub(right),
        ArithmeticOperator::Multiply => left.checked_mul(right),
        ArithmeticOperator::Divide => left.checked_div(right),
        ArithmeticOperator::Remainder => left.checked_rem(right),
    };
    result.ok_or(FoldError::Overflow)
}

/// Arithmetic on two values of `ty`. The result has to fit `ty`, exactly as at run time.
pub(super) fn fold_arithmetic(
    operator: ArithmeticOperator,
    left: i128,
    right: i128,
    ty: IntegerType,
) -> Result<i128, FoldError> {
    let result = compute(operator, left, right)?;
    if ty.contains(result) {
        Ok(result)
    } else {
        Err(FoldError::Overflow)
    }
}

pub(super) fn fold_negate(value: i128, ty: IntegerType) -> Result<i128, FoldError> {
    value
        .checked_neg()
        .filter(|negated| ty.contains(*negated))
        .ok_or(FoldError::Overflow)
}

/// Whether an expression is made only of literals of one kind, so its type comes from its context.
pub(super) fn is_literal(expr: &ast::Expr<'_>) -> bool {
    literal_integer(expr).is_some() || is_float_literal(expr)
}

/// Whether an expression is made only of float literals, `-` and arithmetic other than `%`.
fn is_float_literal(expr: &ast::Expr<'_>) -> bool {
    match &expr.kind {
        ast::ExprKind::Float(_) => true,
        ast::ExprKind::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => is_float_literal(operand),
        ast::ExprKind::Binary {
            operator,
            left,
            right,
        } => {
            arithmetic_operator(*operator)
                .is_some_and(|operator| operator != ArithmeticOperator::Remainder)
                && is_float_literal(left)
                && is_float_literal(right)
        }
        _ => false,
    }
}

/**
 * The value, in type `ty`, of an expression made only of float literals, `-`
 * and arithmetic other than `%`, which floats do not have. `text` gives the
 * source of a span: a `float32` literal is rounded once from its digits, as
 * rounding through `float64` first can miss the nearest `float32`. Returns
 * `None` for any other expression.
 */
pub(super) fn literal_float<'text>(
    expr: &ast::Expr<'_>,
    ty: FloatType,
    text: &impl Fn(Span) -> &'text str,
) -> Option<Result<f64, FloatLiteralError>> {
    match &expr.kind {
        ast::ExprKind::Float(value) => {
            Some(float_literal_value(text(expr.span), *value, ty, expr.span))
        }
        ast::ExprKind::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => literal_float(operand, ty, text).map(|value| value.map(|value| -value)),
        ast::ExprKind::Binary {
            operator,
            left,
            right,
        } => {
            let operator = arithmetic_operator(*operator)
                .filter(|operator| *operator != ArithmeticOperator::Remainder)?;
            let left_value = literal_float(left, ty, text)?;
            let right_value = literal_float(right, ty, text)?;
            Some(left_value.and_then(|left_value| {
                let right_value = right_value?;
                fold_float(operator, left_value, right_value, ty).map_err(|error| match error {
                    FoldError::DivisionByZero => FloatLiteralError::DivisionByZero(right.span),
                    FoldError::Overflow => FloatLiteralError::TooLarge(expr.span),
                })
            }))
        }
        _ => None,
    }
}

/// The float a literal written as `written` stands for in type `ty`. `value` is its nearest `float64`.
pub(super) fn float_literal_value(
    written: &str,
    value: f64,
    ty: FloatType,
    span: Span,
) -> Result<f64, FloatLiteralError> {
    let rounded = match ty {
        FloatType::Float64 => value,
        FloatType::Float32 => written
            .replace('_', "")
            .parse::<f32>()
            .map_or(f64::INFINITY, f64::from),
    };
    if !rounded.is_finite() {
        return Err(FloatLiteralError::TooLarge(span));
    }
    let digits = written.split('e').next().unwrap_or(written);
    let written_zero = !digits.bytes().any(|byte| (b'1'..=b'9').contains(&byte));
    if rounded == 0.0 && !written_zero {
        return Err(FloatLiteralError::TooSmall(span));
    }
    Ok(rounded)
}

/// Arithmetic on two floats of `ty`, rounded to `ty` as the program rounds it. The result has to be a finite number.
pub(super) fn fold_float(
    operator: ArithmeticOperator,
    left: f64,
    right: f64,
    ty: FloatType,
) -> Result<f64, FoldError> {
    if operator == ArithmeticOperator::Divide && right == 0.0 {
        return Err(FoldError::DivisionByZero);
    }
    let result = match ty {
        FloatType::Float64 => compute_float(operator, left, right),
        // Every `float32` is a `float64`, and one operation in `float64` rounded to `float32` gives the `float32` result.
        FloatType::Float32 => ty.round(compute_float(operator, left, right)),
    };
    if result.is_finite() {
        Ok(result)
    } else {
        Err(FoldError::Overflow)
    }
}

fn compute_float(operator: ArithmeticOperator, left: f64, right: f64) -> f64 {
    match operator {
        ArithmeticOperator::Add => left + right,
        ArithmeticOperator::Subtract => left - right,
        ArithmeticOperator::Multiply => left * right,
        ArithmeticOperator::Divide => left / right,
        ArithmeticOperator::Remainder => left % right,
    }
}

/// Compares two known values of the same type. Returns `None` for a comparison the type does not support.
pub(super) fn fold_compare(
    operator: ComparisonOperator,
    left: &Value,
    right: &Value,
) -> Option<bool> {
    let ordering = match (left, right) {
        (Value::Integer { value: left, .. }, Value::Integer { value: right, .. }) => {
            left.cmp(right)
        }
        // Known floats are finite numbers, so they always compare.
        (Value::Float { bits: left, .. }, Value::Float { bits: right, .. }) => {
            f64::from_bits(*left).partial_cmp(&f64::from_bits(*right))?
        }
        (Value::Boolean(left), Value::Boolean(right)) if operator.is_equality() => left.cmp(right),
        // Text orders by its UTF-8 bytes, as the generated code compares it.
        (Value::Text(left), Value::Text(right)) => left.as_bytes().cmp(right.as_bytes()),
        _ => return None,
    };
    Some(match operator {
        ComparisonOperator::Equal => ordering.is_eq(),
        ComparisonOperator::NotEqual => ordering.is_ne(),
        ComparisonOperator::Less => ordering.is_lt(),
        ComparisonOperator::LessEqual => ordering.is_le(),
        ComparisonOperator::Greater => ordering.is_gt(),
        ComparisonOperator::GreaterEqual => ordering.is_ge(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::analyze;
    use crate::source::SourceFile;

    /// The literal value of the expression in `constant x = <expression>`.
    fn literal(expression: &str) -> Option<Result<i128, LiteralError>> {
        let source = SourceFile::new("test.lym", format!("constant x = {expression}\n"))
            .expect("test sources are small");
        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty(), "{expression} should parse");
        match &analysis.module.items.first().map(|item| &item.kind) {
            Some(ast::ItemKind::Constant(binding)) => literal_integer(&binding.value),
            _ => None,
        }
    }

    #[test]
    fn literals_are_computed_exactly() {
        assert_eq!(literal("200 + 100"), Some(Ok(300)));
        assert_eq!(literal("-128"), Some(Ok(-128)));
        assert_eq!(literal("(1 + 2) * -3"), Some(Ok(-9)));
        assert_eq!(literal("7 / 2"), Some(Ok(3)));
        assert_eq!(literal("-7 / 2"), Some(Ok(-3)));
        assert_eq!(literal("-7 % 2"), Some(Ok(-1)));
        assert_eq!(
            literal("18446744073709551615 * 2"),
            Some(Ok(36_893_488_147_419_103_230))
        );
    }

    #[test]
    fn expressions_with_names_or_comparisons_are_not_literals() {
        assert_eq!(literal("count + 1"), None);
        assert_eq!(literal("1 < 2"), None);
        assert_eq!(literal("true"), None);
    }

    #[test]
    fn literal_errors_point_at_their_cause() {
        assert!(matches!(
            literal("1 / 0"),
            Some(Err(LiteralError::DivisionByZero(_)))
        ));
        assert!(matches!(
            literal("1 % (2 - 2)"),
            Some(Err(LiteralError::DivisionByZero(_)))
        ));
        let huge = "18446744073709551615 * 18446744073709551615";
        assert!(matches!(
            literal(huge),
            Some(Err(LiteralError::TooLarge(_)))
        ));
    }

    #[test]
    fn typed_arithmetic_respects_the_range() {
        let add = ArithmeticOperator::Add;
        assert_eq!(
            fold_arithmetic(add, 100, 27, IntegerType::Integer8),
            Ok(127)
        );
        assert_eq!(
            fold_arithmetic(add, 100, 28, IntegerType::Integer8),
            Err(FoldError::Overflow)
        );
        let divide = ArithmeticOperator::Divide;
        assert_eq!(
            fold_arithmetic(divide, -128, -1, IntegerType::Integer8),
            Err(FoldError::Overflow)
        );
        assert_eq!(
            fold_arithmetic(divide, 1, 0, IntegerType::Unsigned8),
            Err(FoldError::DivisionByZero)
        );
        assert_eq!(
            fold_negate(-128, IntegerType::Integer8),
            Err(FoldError::Overflow)
        );
        assert_eq!(fold_negate(5, IntegerType::Integer8), Ok(-5));
    }

    #[test]
    fn comparisons_follow_the_type() {
        let one = Value::Integer {
            value: 1,
            ty: IntegerType::Integer64,
        };
        let two = Value::Integer {
            value: 2,
            ty: IntegerType::Integer64,
        };
        assert_eq!(
            fold_compare(ComparisonOperator::Less, &one, &two),
            Some(true)
        );
        let yes = Value::Boolean(true);
        assert_eq!(
            fold_compare(ComparisonOperator::NotEqual, &yes, &yes),
            Some(false)
        );
        assert_eq!(fold_compare(ComparisonOperator::Less, &yes, &yes), None);
    }
}
