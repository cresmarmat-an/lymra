/*!
 * Expressions: literals, names, operators and calls, with their types.
 *
 * Checking is bidirectional in one small way. An integer literal has no type
 * of its own; it takes the type its context asks for, and `integer64` when
 * nothing asks. Every other expression has exactly one type, and Lymra never
 * converts between types by itself.
 */

use super::errors::{self, described};
use super::features::Feature;
use super::items::is_sized_integer_name;
use super::scope::{Local, LocalKind};
use super::values::{self, FoldError, LiteralError};
use super::{BuiltIn, CheckResult, Checker, DISCARD, Global};
use crate::ast::{self, ArgumentKind, BinaryOperator, UnaryOperator};
use crate::diagnostics::{Diagnostic, ErrorReported, Help};
use crate::foreign_types::translate_type;
use crate::ir::{
    self, ArithmeticOperator, ComparisonOperator, IntegerType, LogicOperator, Type, Value,
};
use crate::source::Span;

/// A checked call: one whose result is a value, or one that only does something.
pub(super) enum CheckedCall<'src> {
    Value(ir::Expr),
    Effect {
        statement: ir::Statement,
        function: &'src str,
    },
}

/// The three families of binary operators, which follow different type rules.
enum OperatorClass {
    Logic(LogicOperator),
    Arithmetic(ArithmeticOperator),
    Comparison(ComparisonOperator),
}

fn classify(operator: BinaryOperator) -> OperatorClass {
    match operator {
        BinaryOperator::And => OperatorClass::Logic(LogicOperator::And),
        BinaryOperator::Or => OperatorClass::Logic(LogicOperator::Or),
        BinaryOperator::Add => OperatorClass::Arithmetic(ArithmeticOperator::Add),
        BinaryOperator::Subtract => OperatorClass::Arithmetic(ArithmeticOperator::Subtract),
        BinaryOperator::Multiply => OperatorClass::Arithmetic(ArithmeticOperator::Multiply),
        BinaryOperator::Divide => OperatorClass::Arithmetic(ArithmeticOperator::Divide),
        BinaryOperator::Remainder => OperatorClass::Arithmetic(ArithmeticOperator::Remainder),
        BinaryOperator::Equal => OperatorClass::Comparison(ComparisonOperator::Equal),
        BinaryOperator::NotEqual => OperatorClass::Comparison(ComparisonOperator::NotEqual),
        BinaryOperator::Less => OperatorClass::Comparison(ComparisonOperator::Less),
        BinaryOperator::LessEqual => OperatorClass::Comparison(ComparisonOperator::LessEqual),
        BinaryOperator::Greater => OperatorClass::Comparison(ComparisonOperator::Greater),
        BinaryOperator::GreaterEqual => OperatorClass::Comparison(ComparisonOperator::GreaterEqual),
    }
}

fn literal(value: Value, span: Span) -> ir::Expr {
    ir::Expr {
        ty: value.ty(),
        kind: ir::ExprKind::Literal(value),
        span,
    }
}

fn binary(kind: ir::ExprKind, ty: Type, span: Span) -> ir::Expr {
    ir::Expr { kind, ty, span }
}

impl<'src, 'ast> Checker<'src, 'ast> {
    /// Checks an expression that has to be of type `expected`, when one is given.
    pub(super) fn check_expr(
        &mut self,
        expr: &ast::Expr<'src>,
        expected: Option<&Type>,
    ) -> CheckResult<ir::Expr> {
        let checked = self.check_expr_with_hint(expr, expected)?;
        match expected {
            Some(expected) if checked.ty != *expected => {
                // A literal has no type of its own until a context gives it one, so it is just "a number" here.
                let error = match (expected, &checked.ty) {
                    _ if values::literal_integer(expr).is_some() => {
                        errors::literal_mismatch(expected, expr.span)
                    }
                    (Type::Integer(target), Type::Integer(_)) => {
                        let source = self.text(expr.span);
                        errors::integer_mismatch(*target, &checked.ty, expr.span, source)
                    }
                    _ => errors::type_mismatch(expected, &checked.ty, expr.span),
                };
                Err(self.report(error))
            }
            _ => Ok(checked),
        }
    }

    /// Checks an expression. `hint` gives integer literals their type and is otherwise ignored.
    pub(super) fn check_expr_with_hint(
        &mut self,
        expr: &ast::Expr<'src>,
        hint: Option<&Type>,
    ) -> CheckResult<ir::Expr> {
        if let Some(value) = values::literal_integer(expr) {
            return self.integer_literal(expr, value, hint);
        }
        match &expr.kind {
            ast::ExprKind::Integer(value) => {
                self.integer_literal(expr, Ok(i128::from(*value)), hint)
            }
            ast::ExprKind::Float(_) => Err(self.report(Feature::Floats.diagnostic(expr.span))),
            ast::ExprKind::Bool(value) => Ok(literal(Value::Boolean(*value), expr.span)),
            ast::ExprKind::Text(parts) => self.check_text(parts, expr.span),
            ast::ExprKind::Name(name) => self.check_name(name, expr.span),
            ast::ExprKind::Unary { operator, operand } => {
                self.check_unary(*operator, operand, expr.span, hint)
            }
            ast::ExprKind::Binary {
                operator,
                left,
                right,
            } => self.check_binary(*operator, left, right, expr.span),
            ast::ExprKind::Call(call) => match self.check_call(call, expr.span, hint)? {
                CheckedCall::Value(value) => Ok(value),
                CheckedCall::Effect { function, .. } => {
                    Err(self.report(errors::no_value(function, expr.span)))
                }
            },
            ast::ExprKind::Field { object, .. } => Err(self.reject_field(object, expr.span)),
            ast::ExprKind::Index { object, index } => self.check_index(object, index, expr.span),
        }
    }

    /**
     * Checks `value` against the type of the place it goes to, if there is
     * one. When that type failed to resolve, which was reported already, the
     * value is only checked for mistakes of its own: an `array(value)` there
     * has no length to take, and saying so would report the same mistake twice.
     */
    pub(super) fn check_against(
        &mut self,
        value: &ast::Expr<'src>,
        expected: Option<Result<&Type, &ErrorReported>>,
    ) -> CheckResult<ir::Expr> {
        match expected {
            None => self.check_expr(value, None),
            Some(Ok(ty)) => self.check_expr(value, Some(ty)),
            Some(Err(ErrorReported)) => {
                self.check_untyped(value);
                Err(ErrorReported)
            }
        }
    }

    /// Checks a value whose type failed to resolve for its own mistakes. See `check_against`.
    pub(super) fn check_untyped(&mut self, value: &ast::Expr<'src>) {
        match &value.kind {
            ast::ExprKind::Call(call)
                if matches!(call.callee.kind, ast::ExprKind::Name("array")) =>
            {
                for argument in &call.arguments {
                    match &argument.kind {
                        ArgumentKind::Positional(inner)
                        | ArgumentKind::Mutable(inner)
                        | ArgumentKind::Named { value: inner, .. } => self.check_untyped(inner),
                    }
                }
            }
            _ => self.check_for_errors(value),
        }
    }

    /**
     * Checks an expression for its own errors and drops the result. Used
     * where the statement around it is rejected for another reason, so the
     * expression's own mistakes are still reported and its bindings count
     * as read.
     */
    pub(super) fn check_for_errors(&mut self, expr: &ast::Expr<'src>) {
        // Errors inside are reported by the check itself; there is no value to use.
        let _ = self.check_expr_with_hint(expr, None);
    }

    fn integer_literal(
        &mut self,
        expr: &ast::Expr<'src>,
        value: Result<i128, LiteralError>,
        hint: Option<&Type>,
    ) -> CheckResult<ir::Expr> {
        let value = match value {
            Ok(value) => value,
            Err(LiteralError::DivisionByZero(divisor)) => {
                return Err(self.report(errors::division_by_zero(divisor)));
            }
            Err(LiteralError::TooLarge(span)) => {
                return Err(self.report(errors::literal_too_large(span)));
            }
        };
        let ty = hint
            .and_then(Type::as_integer)
            .unwrap_or(IntegerType::DEFAULT);
        if !ty.contains(value) {
            let text = self.text(expr.span);
            let shown = if text == value.to_string() {
                format!("`{text}`")
            } else {
                format!("`{text}`, which is {value},")
            };
            return Err(self.report(errors::out_of_range(&shown, ty, expr.span)));
        }
        Ok(literal(Value::Integer { value, ty }, expr.span))
    }

    /// Text without interpolation is a value. Text with interpolation can only be printed for now.
    fn check_text(&mut self, parts: &[ast::TextPart<'src>], span: Span) -> CheckResult<ir::Expr> {
        let mut text = String::new();
        let mut interpolated = false;
        for part in parts {
            match part {
                ast::TextPart::Literal(literal) => text.push_str(literal),
                ast::TextPart::Interpolation(expr) => {
                    self.mark_reads(expr);
                    interpolated = true;
                }
            }
        }
        if interpolated {
            return Err(self.report(Feature::TextBuilding.diagnostic(span)));
        }
        Ok(literal(Value::Text(text), span))
    }

    pub(super) fn check_name(&mut self, name: &'src str, span: Span) -> CheckResult<ir::Expr> {
        if name == DISCARD {
            return Err(self.report(errors::discard_read(span)));
        }
        if let Some(scope) = self.scope.as_mut() {
            if let Some(id) = scope.lookup(name) {
                let Some(local) = scope.local_mut(id) else {
                    return Err(ErrorReported);
                };
                local.read = true;
                let ty = local.ty.clone()?;
                return Ok(ir::Expr {
                    kind: ir::ExprKind::Local(id),
                    ty,
                    span,
                });
            }
        }
        match self.globals.get(name).copied() {
            Some(Global::Constant(id)) => {
                if let Some(entry) = self.constants.get_mut(id) {
                    entry.used = true;
                }
                let value = self.constant_value(id, Some(span))?;
                Ok(ir::Expr {
                    kind: ir::ExprKind::Constant(id),
                    ty: value.ty(),
                    span,
                })
            }
            Some(Global::Function(_)) => Err(self.report(errors::function_as_value(name, span))),
            None => match BuiltIn::of(name) {
                Some(BuiltIn::Function) => Err(self.report(errors::function_as_value(name, span))),
                Some(BuiltIn::Type) => Err(self.report(errors::type_as_value(name, span))),
                None => match Feature::future_function(name) {
                    Some(feature) => Err(self.report(feature.diagnostic(span))),
                    None => Err(self.report_unknown_name(name, span)),
                },
            },
        }
    }

    fn check_unary(
        &mut self,
        operator: UnaryOperator,
        operand: &ast::Expr<'src>,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<ir::Expr> {
        let operand = self.check_expr_with_hint(operand, hint)?;
        match operator {
            UnaryOperator::Not => {
                if operand.ty != Type::Boolean {
                    return Err(self.report(errors::operator_type(
                        format!(
                            "`not` works on `boolean` values, but this is {}",
                            described(&operand.ty)
                        ),
                        operand.span,
                        "not a `boolean`".to_owned(),
                        "compare it to get a `boolean`, for example `count == 0`",
                    )));
                }
                if let Some(Value::Boolean(value)) = self.known_value(&operand) {
                    return Ok(literal(Value::Boolean(!value), span));
                }
                Ok(binary(
                    ir::ExprKind::Not(Box::new(operand)),
                    Type::Boolean,
                    span,
                ))
            }
            UnaryOperator::Negate => {
                let ty = self.negatable_type(&operand)?;
                if let Some(Value::Integer { value, .. }) = self.known_value(&operand) {
                    return match values::fold_negate(value, ty) {
                        Ok(negated) => Ok(literal(Value::Integer { value: negated, ty }, span)),
                        Err(_) => Err(self.report(errors::always_overflows(ty, span))),
                    };
                }
                let kind = ir::ExprKind::Negate {
                    operand: Box::new(operand),
                    checked: true,
                };
                Ok(binary(kind, Type::Integer(ty), span))
            }
        }
    }

    fn negatable_type(&mut self, operand: &ir::Expr) -> CheckResult<IntegerType> {
        let (message, help) = match &operand.ty {
            Type::Integer(ty) if ty.is_signed() => return Ok(*ty),
            Type::Integer(ty) => (
                format!(
                    "`-` cannot negate an unsigned value, and this is {}",
                    described(&operand.ty)
                ),
                format!(
                    "use a signed type such as `{}` if the value can be negative",
                    signed_counterpart(*ty).name()
                ),
            ),
            Type::Boolean => (
                "`-` works on numbers, but this is a `boolean`".to_owned(),
                "to flip a `boolean`, use `not`".to_owned(),
            ),
            Type::String | Type::Array(_) => (
                format!(
                    "`-` works on numbers, but this is {}",
                    described(&operand.ty)
                ),
                "only numbers can be negated".to_owned(),
            ),
        };
        let label = format!("this is {}", described(&operand.ty));
        Err(self.report(errors::operator_type(message, operand.span, label, &help)))
    }

    fn check_binary(
        &mut self,
        operator: BinaryOperator,
        left: &ast::Expr<'src>,
        right: &ast::Expr<'src>,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        match classify(operator) {
            OperatorClass::Logic(logic) => self.check_logic(logic, left, right, span),
            OperatorClass::Arithmetic(arithmetic) => {
                let (left, right) = self.check_operands(left, right)?;
                self.check_arithmetic(arithmetic, left, right, span)
            }
            OperatorClass::Comparison(comparison) => {
                let (left, right) = self.check_operands(left, right)?;
                self.check_comparison(comparison, left, right, span)
            }
        }
    }

    /// Checks both operands. An integer literal on one side takes the type of the other side.
    pub(super) fn check_operands(
        &mut self,
        left: &ast::Expr<'src>,
        right: &ast::Expr<'src>,
    ) -> CheckResult<(ir::Expr, ir::Expr)> {
        let left_is_literal = values::literal_integer(left).is_some();
        let right_is_literal = values::literal_integer(right).is_some();
        if left_is_literal && !right_is_literal {
            let right = self.check_expr_with_hint(right, None);
            let hint = right.as_ref().ok().map(|right| &right.ty);
            let left = self.check_expr_with_hint(left, hint);
            return Ok((left?, right?));
        }
        let left = self.check_expr_with_hint(left, None);
        let hint = if right_is_literal {
            left.as_ref().ok().map(|left| &left.ty)
        } else {
            None
        };
        let right = self.check_expr_with_hint(right, hint);
        Ok((left?, right?))
    }

    /// The integer type both operands share. `symbol` names the operation in messages.
    pub(super) fn shared_integer_type(
        &mut self,
        symbol: &str,
        left: &ir::Expr,
        right: &ir::Expr,
    ) -> CheckResult<IntegerType> {
        let mut failed = false;
        for operand in [left, right] {
            if operand.ty.as_integer().is_none() {
                let help = match operand.ty {
                    Type::String => {
                        "to put text together, interpolate it: `print(\"{first}{second}\")`"
                    }
                    Type::Array(_) => {
                        "arithmetic works on single numbers; work on the elements one by one"
                    }
                    Type::Boolean | Type::Integer(_) => {
                        "arithmetic needs integer types such as `integer64`"
                    }
                };
                self.report(errors::operator_type(
                    format!(
                        "`{symbol}` works on numbers, but this is {}",
                        described(&operand.ty)
                    ),
                    operand.span,
                    "not a number".to_owned(),
                    help,
                ));
                failed = true;
            }
        }
        if failed {
            return Err(ErrorReported);
        }
        match (&left.ty, &right.ty) {
            (Type::Integer(shared), Type::Integer(other)) if shared == other => Ok(*shared),
            _ => Err(self.report(errors::operand_mismatch(
                symbol,
                (&left.ty, left.span),
                (&right.ty, right.span),
            ))),
        }
    }

    fn check_arithmetic(
        &mut self,
        operator: ArithmeticOperator,
        left: ir::Expr,
        right: ir::Expr,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        let ty = self.shared_integer_type(operator.symbol(), &left, &right)?;
        let divides = matches!(
            operator,
            ArithmeticOperator::Divide | ArithmeticOperator::Remainder
        );
        let left_value = self.known_value(&left);
        let right_value = self.known_value(&right);
        if divides && matches!(right_value, Some(Value::Integer { value: 0, .. })) {
            return Err(self.report(errors::division_by_zero(right.span)));
        }
        if let (
            Some(Value::Integer {
                value: left_value, ..
            }),
            Some(Value::Integer {
                value: right_value, ..
            }),
        ) = (left_value, right_value)
        {
            return match values::fold_arithmetic(operator, left_value, right_value, ty) {
                Ok(value) => Ok(literal(Value::Integer { value, ty }, span)),
                Err(FoldError::Overflow) => Err(self.report(errors::always_overflows(ty, span))),
                Err(FoldError::DivisionByZero) => {
                    Err(self.report(errors::division_by_zero(right.span)))
                }
            };
        }
        let kind = ir::ExprKind::Arithmetic {
            operator,
            left: Box::new(left),
            right: Box::new(right),
            checked: true,
        };
        Ok(binary(kind, Type::Integer(ty), span))
    }

    fn check_comparison(
        &mut self,
        operator: ComparisonOperator,
        left: ir::Expr,
        right: ir::Expr,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        let symbol = operator.symbol();
        if left.ty != right.ty {
            return Err(self.report(errors::operand_mismatch(
                symbol,
                (&left.ty, left.span),
                (&right.ty, right.span),
            )));
        }
        match &left.ty {
            Type::Boolean if !operator.is_equality() => {
                return Err(self.report(errors::operator_type(
                    format!("`{symbol}` compares numbers, and a `boolean` has no order"),
                    span,
                    "comparing `boolean` values".to_owned(),
                    "compare `boolean` values with `==` and `!=`",
                )));
            }
            Type::Array(_) => {
                return Err(self.report(errors::operator_type(
                    format!("`{symbol}` compares single values, and these are arrays"),
                    span,
                    "comparing arrays".to_owned(),
                    "compare the elements one by one in a loop",
                )));
            }
            Type::Integer(_) | Type::Boolean | Type::String => {}
        }
        if let (Some(left_value), Some(right_value)) =
            (self.known_value(&left), self.known_value(&right))
        {
            if let Some(result) = values::fold_compare(operator, &left_value, &right_value) {
                return Ok(literal(Value::Boolean(result), span));
            }
        }
        let kind = ir::ExprKind::Compare {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        };
        Ok(binary(kind, Type::Boolean, span))
    }

    fn check_logic(
        &mut self,
        operator: LogicOperator,
        left: &ast::Expr<'src>,
        right: &ast::Expr<'src>,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        let symbol = match operator {
            LogicOperator::And => "and",
            LogicOperator::Or => "or",
        };
        let left = self.check_expr_with_hint(left, None);
        let right = self.check_expr_with_hint(right, None);
        let (left, right) = (left?, right?);
        let mut failed = false;
        for operand in [&left, &right] {
            if operand.ty != Type::Boolean {
                self.report(errors::operator_type(
                    format!(
                        "`{symbol}` works on `boolean` values, but this is {}",
                        described(&operand.ty)
                    ),
                    operand.span,
                    "not a `boolean`".to_owned(),
                    "compare it to get a `boolean`, for example `count != 0`",
                ));
                failed = true;
            }
        }
        if failed {
            return Err(ErrorReported);
        }
        if let (Some(Value::Boolean(left_value)), Some(Value::Boolean(right_value))) =
            (self.known_value(&left), self.known_value(&right))
        {
            let result = match operator {
                LogicOperator::And => left_value && right_value,
                LogicOperator::Or => left_value || right_value,
            };
            return Ok(literal(Value::Boolean(result), span));
        }
        let kind = ir::ExprKind::Logic {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        };
        Ok(binary(kind, Type::Boolean, span))
    }

    /// Reports `object.field`: a field of a value, or a member of a module.
    pub(super) fn reject_field(&mut self, object: &ast::Expr<'src>, span: Span) -> ErrorReported {
        if let ast::ExprKind::Name(name) = &object.kind {
            let is_value = self
                .scope
                .as_ref()
                .is_some_and(|scope| scope.lookup(name).is_some())
                || matches!(self.globals.get(name), Some(Global::Constant(_)));
            if !is_value {
                return self.report(Feature::Modules.diagnostic(span));
            }
        }
        self.mark_reads(object);
        self.report(Feature::Fields.diagnostic(span))
    }

    /// Checks a call. `hint` is the type the place the result goes to asks for, which `array(value)` needs.
    pub(super) fn check_call(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<CheckedCall<'src>> {
        if let Some(block) = &call.block {
            self.mark_argument_reads(&call.arguments);
            let with = Span::new(block.span.start, block.span.start + "with".len() as u32);
            return Err(self.report(Feature::CodeArguments.diagnostic(with)));
        }
        let callee = &call.callee;
        let name = match &callee.kind {
            ast::ExprKind::Name(name) => *name,
            ast::ExprKind::Field { .. } => {
                self.mark_argument_reads(&call.arguments);
                return Err(self.report(Feature::Modules.diagnostic(callee.span)));
            }
            _ => return Err(self.report(errors::not_callable(callee.span))),
        };
        if self
            .scope
            .as_ref()
            .is_some_and(|scope| scope.lookup(name).is_some())
        {
            // A local with a built-in name was rejected where it was declared; the call means the built-in.
            if BuiltIn::of(name).is_some() {
                self.mark_argument_reads(&call.arguments);
                return Err(ErrorReported);
            }
            return Err(self.report(errors::not_a_function(name, callee.span)));
        }
        match self.globals.get(name).copied() {
            Some(Global::Function(id)) => self.check_function_call(id, name, call, span),
            Some(Global::Constant(_)) => {
                Err(self.report(errors::not_a_function(name, callee.span)))
            }
            None => self.check_undeclared_call(name, call, span, hint),
        }
    }

    /**
     * A call to a name the file does not declare: a built-in function, a
     * conversion such as `integer32(count)`, or a mistake that gets the
     * spelling it most likely meant.
     */
    fn check_undeclared_call(
        &mut self,
        name: &'src str,
        call: &ast::Call<'src>,
        span: Span,
        hint: Option<&Type>,
    ) -> CheckResult<CheckedCall<'src>> {
        if let Some(target) = IntegerType::from_name(name) {
            return self.check_conversion(target, name, call, span);
        }
        match name {
            "print" => return self.check_print(call, span),
            "array" => return self.check_fill(call, span, hint),
            "length" => return self.check_length(call, span),
            _ => {}
        }
        if let Some(operator) = ir::WrappingOperator::from_function(name) {
            return self.check_wrapping(operator, call, span, hint);
        }
        self.mark_argument_reads(&call.arguments);
        // A `type` or `choice` of a later phase is already reported where it is declared.
        if self.unavailable_types.contains(name) {
            return Err(ErrorReported);
        }
        let callee = call.callee.span;
        let diagnostic = match name {
            "range" => Feature::RangeValues.diagnostic(span),
            "boolean" | "string" => errors::no_conversion(name, callee),
            "integer" | "unsigned" => errors::sizeless_integer(name, callee),
            _ if is_sized_integer_name(name) => errors::unsupported_integer_size(name, callee),
            _ => {
                let later = Feature::future_function(name).or_else(|| Feature::future_type(name));
                match (later, translate_type(name)) {
                    (Some(feature), _) => feature.diagnostic(callee),
                    (None, Some(translation)) => translation.diagnostic(name, callee),
                    (None, None) => return Err(self.report_unknown_callee(name, callee)),
                }
            }
        };
        Err(self.report(diagnostic))
    }

    /**
     * Checks a conversion such as `unsigned8(count)`. It is checked at
     * run time only when some value of the operand's type does not fit the
     * target type; a known value is converted now.
     */
    fn check_conversion(
        &mut self,
        target: IntegerType,
        name: &'src str,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let [argument] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::conversion_argument_count(
                name,
                call.arguments.len(),
                arguments,
            )));
        };
        let value = self.single_value_argument(argument, "value")?;
        // A literal takes the target type directly, so it is checked against the target's range.
        let operand = self.check_expr_with_hint(value, Some(&Type::Integer(target)))?;
        let Type::Integer(source) = operand.ty else {
            return Err(self.report(errors::operator_type(
                format!(
                    "`{name}` converts integers, but this is {}",
                    described(&operand.ty)
                ),
                operand.span,
                "not an integer".to_owned(),
                "only integers can be converted between integer types",
            )));
        };
        if let Some(Value::Integer { value: known, .. }) = self.known_value(&operand) {
            if !target.contains(known) {
                let shown = format!("`{}`, which is {known},", self.text(value.span));
                return Err(self.report(errors::out_of_range(&shown, target, value.span)));
            }
            return Ok(CheckedCall::Value(literal(
                Value::Integer {
                    value: known,
                    ty: target,
                },
                span,
            )));
        }
        let checked = !(target.contains(source.minimum()) && target.contains(source.maximum()));
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Convert {
                operand: Box::new(operand),
                checked,
            },
            ty: Type::Integer(target),
            span,
        }))
    }

    /**
     * The value of the one argument of a built-in function, named `parameter`.
     * It has to be plain: not passed with `variable`, and not named.
     */
    pub(super) fn single_value_argument<'argument>(
        &mut self,
        argument: &'argument ast::Argument<'src>,
        parameter: &str,
    ) -> CheckResult<&'argument ast::Expr<'src>> {
        match &argument.kind {
            ArgumentKind::Positional(value) => Ok(value),
            ArgumentKind::Mutable(value) => {
                self.check_for_errors(value);
                let mark = Span::new(argument.span.start, value.span.start);
                Err(self.report(errors::needless_variable_mark(parameter, mark)))
            }
            ArgumentKind::Named { value, .. } => {
                self.mark_reads(value);
                Err(self.report(Feature::NamedArguments.diagnostic(argument.span)))
            }
        }
    }

    /// Marks what the arguments of a rejected call read, without reporting anything inside them.
    pub(super) fn mark_argument_reads(&mut self, arguments: &[ast::Argument<'src>]) {
        for argument in arguments {
            match &argument.kind {
                ArgumentKind::Positional(value) | ArgumentKind::Mutable(value) => {
                    self.mark_reads(value)
                }
                ArgumentKind::Named { value, .. } => self.mark_reads(value),
            }
        }
    }

    pub(super) fn check_arguments_for_errors(&mut self, arguments: &[ast::Argument<'src>]) {
        for argument in arguments {
            match &argument.kind {
                ArgumentKind::Positional(value) | ArgumentKind::Mutable(value) => {
                    self.check_for_errors(value)
                }
                ArgumentKind::Named { value, .. } => self.check_for_errors(value),
            }
        }
    }

    fn check_function_call(
        &mut self,
        id: ir::FunctionId,
        name: &'src str,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let Some(entry) = self.functions.get(id) else {
            return Err(ErrorReported);
        };
        let declaration = entry.declaration;
        let parameters = entry.parameters.clone();
        let return_type = entry.return_type.clone();

        if call.arguments.len() != parameters.len() {
            self.check_arguments_for_errors(&call.arguments);
            let signature = self.signature_text(name, &parameters);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::argument_count(
                name,
                parameters.len(),
                call.arguments.len(),
                arguments,
                declaration.name.span,
                &signature,
            )));
        }

        if let Some(named) = call
            .arguments
            .iter()
            .find(|argument| matches!(argument.kind, ArgumentKind::Named { .. }))
        {
            self.mark_argument_reads(&call.arguments);
            return Err(self.report(Feature::NamedArguments.diagnostic(named.span)));
        }

        let mut arguments = Vec::new();
        let mut failed = false;
        for (argument, parameter) in call.arguments.iter().zip(&parameters) {
            match self.check_argument(argument, parameter) {
                Ok(checked) => arguments.push(checked),
                Err(ErrorReported) => failed = true,
            }
        }
        if failed {
            return Err(ErrorReported);
        }
        let call = ir::Call {
            function: id,
            arguments,
        };
        Ok(match return_type? {
            Some(ty) => CheckedCall::Value(ir::Expr {
                kind: ir::ExprKind::Call(call),
                ty,
                span,
            }),
            None => CheckedCall::Effect {
                statement: ir::Statement::Call(call),
                function: name,
            },
        })
    }

    /// The function's name and parameters as declared, as in `area(integer64 width, integer64 height)`.
    fn signature_text(&self, name: &str, parameters: &[super::Parameter<'src>]) -> String {
        let parameters: Vec<&str> = parameters
            .iter()
            .map(|parameter| self.text(parameter.span))
            .collect();
        format!("{name}({})", parameters.join(", "))
    }

    fn check_argument(
        &mut self,
        argument: &ast::Argument<'src>,
        parameter: &super::Parameter<'src>,
    ) -> CheckResult<ir::Argument> {
        match (&argument.kind, parameter.mutable) {
            (ArgumentKind::Named { value, .. }, _) => {
                self.mark_reads(value);
                Err(self.report(Feature::NamedArguments.diagnostic(argument.span)))
            }
            (ArgumentKind::Positional(value), false) => self
                .check_against(value, Some(parameter.ty.as_ref()))
                .map(ir::Argument::Value),
            (ArgumentKind::Positional(value), true) => {
                self.check_for_errors(value);
                Err(self.report(errors::missing_variable_mark(
                    parameter.name.text,
                    argument.span,
                )))
            }
            (ArgumentKind::Mutable(place), false) => {
                self.check_for_errors(place);
                let mark = Span::new(argument.span.start, place.span.start);
                Err(self.report(errors::needless_variable_mark(parameter.name.text, mark)))
            }
            (ArgumentKind::Mutable(place), true) => self
                .check_reference(place, parameter.ty.as_ref())
                .map(ir::Argument::Reference),
        }
    }

    /// Checks a binding passed with `variable`, which the called function may change.
    fn check_reference(
        &mut self,
        place: &ast::Expr<'src>,
        expected: Result<&Type, &ErrorReported>,
    ) -> CheckResult<ir::LocalId> {
        let ast::ExprKind::Name(name) = &place.kind else {
            return Err(self.report_unchangeable_place(place));
        };
        let name = *name;
        let local = self.scope.as_ref().and_then(|scope| scope.lookup(name));
        let Some(id) = local else {
            return Err(self.report_change_to_global(name, place.span));
        };
        let Some(local) = self.scope.as_mut().and_then(|scope| scope.local_mut(id)) else {
            return Err(ErrorReported);
        };
        local.read = true;
        if !local.kind.is_mutable() {
            let error = immutable_local_error(local, place.span);
            return Err(self.report(error));
        }
        local.written = true;
        let ty = local.ty.clone()?;
        match expected {
            Ok(expected) if *expected != ty => {
                Err(self.report(errors::type_mismatch(expected, &ty, place.span)))
            }
            Ok(_) => Ok(id),
            Err(ErrorReported) => Err(ErrorReported),
        }
    }

    /// Reports a field or element passed or assigned where only a binding can be.
    pub(super) fn report_unchangeable_place(&mut self, place: &ast::Expr<'src>) -> ErrorReported {
        match &place.kind {
            ast::ExprKind::Field { object, .. } => self.reject_field(object, place.span),
            ast::ExprKind::Index { .. } => {
                self.mark_reads(place);
                self.report(errors::element_reference(place.span))
            }
            _ => self.report(errors::reference_needs_variable(place.span)),
        }
    }

    /// Reports a change to a name that is not a local: a top-level constant, a function, or nothing.
    pub(super) fn report_change_to_global(&mut self, name: &'src str, span: Span) -> ErrorReported {
        match self.globals.get(name).copied() {
            Some(Global::Constant(id)) => {
                let declaration = self.global_span(Global::Constant(id)).unwrap_or(span);
                let help = Help::text(
                    "a top-level constant never changes; declare a `variable` inside the function",
                );
                self.report(errors::immutable_changed(
                    name,
                    "top-level constant",
                    span,
                    declaration,
                    help,
                ))
            }
            Some(Global::Function(_)) => {
                self.report(errors::not_assignable(name, "function", span))
            }
            None => match BuiltIn::of(name) {
                Some(built_in) => self.report(errors::not_assignable(name, built_in.noun(), span)),
                None => self.report_unknown_name(name, span),
            },
        }
    }

    fn check_print(
        &mut self,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let [argument] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::print_argument_count(
                call.arguments.len(),
                arguments,
            )));
        };
        let value = self.single_value_argument(argument, "text")?;
        let parts = match &value.kind {
            ast::ExprKind::Text(parts) => self.check_print_parts(parts)?,
            _ => {
                let checked = self.check_expr_with_hint(value, None)?;
                if checked.ty != Type::String {
                    let source = self.text(value.span);
                    return Err(self.report(errors::print_needs_text(
                        &checked.ty,
                        value.span,
                        source,
                    )));
                }
                vec![ir::PrintPart::Value {
                    value: checked,
                    form: ir::TextForm::Text,
                }]
            }
        };
        Ok(CheckedCall::Effect {
            statement: ir::Statement::Print(parts),
            function: "print",
        })
    }

    /// Interpolated values of every type but arrays can be printed. Each part is checked even after one fails.
    fn check_print_parts(
        &mut self,
        parts: &[ast::TextPart<'src>],
    ) -> CheckResult<Vec<ir::PrintPart>> {
        let mut checked = Vec::new();
        let mut failed = false;
        for part in parts {
            match part {
                ast::TextPart::Literal(text) => checked.push(ir::PrintPart::Text(text.clone())),
                ast::TextPart::Interpolation(expr) => match self.check_expr_with_hint(expr, None) {
                    Ok(value) => match ir::TextForm::of(&value.ty) {
                        Some(form) => checked.push(ir::PrintPart::Value { value, form }),
                        None => {
                            self.report(errors::array_in_text(value.span));
                            failed = true;
                        }
                    },
                    Err(ErrorReported) => failed = true,
                },
            }
        }
        if failed {
            Err(ErrorReported)
        } else {
            Ok(checked)
        }
    }
}

/// The error for changing a local that cannot change, with the fix that fits its kind.
pub(super) fn immutable_local_error(local: &Local<'_>, span: Span) -> Diagnostic {
    let name = local.name.text;
    let help = match (local.kind, local.keyword) {
        (LocalKind::Constant, Some(keyword)) => Help::replace(
            "declare it with `variable` if it has to change",
            keyword,
            "variable",
        ),
        (LocalKind::Parameter, _) => Help::text(format!(
            "copy it into a local first, as in `variable local_{name} = {name}`; \
             or declare the parameter `variable` to change the caller's binding"
        )),
        (LocalKind::LoopVariable, _) => Help::text(format!(
            "copy it into a local first, as in `variable local_{name} = {name}`"
        )),
        _ => Help::text("declare it with `variable` if it has to change"),
    };
    errors::immutable_changed(name, local.kind.noun(), span, local.name.span, help)
}

/// The narrowest signed type that holds every value of `ty`, or `integer64`, the widest, for `unsigned64`.
fn signed_counterpart(ty: IntegerType) -> IntegerType {
    match ty.bits() {
        8 => IntegerType::Integer16,
        16 => IntegerType::Integer32,
        _ => IntegerType::Integer64,
    }
}
