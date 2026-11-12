/*!
 * C for expressions, computed from left to right.
 *
 * C leaves the order of most operands open: the arguments of a call, and the
 * two sides of `+` or `<`. Lymra runs the parts of an expression from left to
 * right (spec section 4.1), so wherever the order could show, the part that
 * comes first is computed into a temporary before the expression, as a
 * statement of its own.
 */

use super::{Generator, INDENT, Writer, c_syntax, local_access};
use crate::ir::{
    Argument, ArithmeticOperator, Call, Expr, ExprKind, Function, IntegerType, LogicOperator, Type,
};

/**
 * Whether computing `expr` can be seen from outside it: a call can print or
 * stop the program, and so can a checked operation. Reading values cannot,
 * and no part of an expression can change a binding another part reads
 * (`L0508`), so the order of parts without effects never shows.
 */
pub(super) fn has_effects(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Local(_) | ExprKind::Constant(_) => false,
        ExprKind::Call(_) => true,
        ExprKind::Not(operand) => has_effects(operand),
        ExprKind::Negate { operand, checked } | ExprKind::Convert { operand, checked } => {
            *checked || has_effects(operand)
        }
        ExprKind::Arithmetic {
            left,
            right,
            checked,
            ..
        } => *checked || has_effects(left) || has_effects(right),
        ExprKind::Compare { left, right, .. } | ExprKind::Logic { left, right, .. } => {
            has_effects(left) || has_effects(right)
        }
    }
}

impl Generator<'_, '_> {
    /// The C for `expr`. Statements it needs first go to `writer`, in the order they have to run.
    pub(super) fn expr(
        &mut self,
        function: &Function<'_>,
        expr: &Expr,
        writer: &mut Writer,
    ) -> String {
        match &expr.kind {
            ExprKind::Literal(value) => c_syntax::value(value),
            ExprKind::Local(id) => local_access(function, *id),
            ExprKind::Constant(id) => c_syntax::constant_name(self.program.constants[*id].name),
            ExprKind::Not(operand) => format!("(!{})", self.expr(function, operand, writer)),
            ExprKind::Negate { operand, checked } => {
                let operand = self.expr(function, operand, writer);
                if !checked {
                    // The optimizer proved the operand is not the type's lowest value.
                    return format!("(({})(-{operand}))", c_syntax::type_name(expr.ty));
                }
                let site = self.site(expr.span);
                format!("lymra_negate_{}({operand}, {site})", expr.ty.name())
            }
            ExprKind::Arithmetic {
                operator,
                left,
                right,
                checked,
            } => {
                let (left, right) = self.pair(function, left, right, writer);
                if !checked {
                    /*
                     * The optimizer proved the result fits the type and no divisor is
                     * zero. C computes small types in `int`, which holds every value
                     * of them, so the cast back loses nothing.
                     */
                    return format!(
                        "(({})({left} {} {right}))",
                        c_syntax::type_name(expr.ty),
                        operator.symbol()
                    );
                }
                let site = self.site(expr.span);
                let operation = match operator {
                    ArithmeticOperator::Add => "add",
                    ArithmeticOperator::Subtract => "subtract",
                    ArithmeticOperator::Multiply => "multiply",
                    ArithmeticOperator::Divide => "divide",
                    ArithmeticOperator::Remainder => "remainder",
                };
                format!(
                    "lymra_{operation}_{}({left}, {right}, {site})",
                    expr.ty.name()
                )
            }
            ExprKind::Compare { .. } => format!("({})", self.condition(function, expr, writer)),
            ExprKind::Logic {
                operator,
                left,
                right,
            } => self.logic(function, *operator, left, right, writer),
            ExprKind::Call(call) => self.call(function, call, writer),
            ExprKind::Convert { operand, checked } => {
                self.convert(function, operand, expr, *checked, writer)
            }
        }
    }

    /**
     * The C for the condition of an `if` or a `while`. A comparison leaves out
     * its outer parentheses there, which C compilers take as a sign that `==`
     * was meant to be `=`.
     */
    pub(super) fn condition(
        &mut self,
        function: &Function<'_>,
        expr: &Expr,
        writer: &mut Writer,
    ) -> String {
        let ExprKind::Compare {
            operator,
            left,
            right,
        } = &expr.kind
        else {
            return self.expr(function, expr, writer);
        };
        let (left, right) = self.pair(function, left, right, writer);
        format!("{left} {} {right}", operator.symbol())
    }

    /// Computes `code`, of type `ty`, into a new constant and returns the constant's name.
    pub(super) fn temporary(&mut self, ty: Type, code: &str, writer: &mut Writer) -> String {
        let name = self.helper("value");
        writer.line(&format!(
            "const {} {name} = {code};",
            c_syntax::type_name(ty)
        ));
        name
    }

    /// The C for the two operands of an operator, the left one computed first when the order could show.
    fn pair(
        &mut self,
        function: &Function<'_>,
        left: &Expr,
        right: &Expr,
        writer: &mut Writer,
    ) -> (String, String) {
        let left_code = self.expr(function, left, writer);
        let left_code = if has_effects(left) && has_effects(right) {
            self.temporary(left.ty, &left_code, writer)
        } else {
            left_code
        };
        (left_code, self.expr(function, right, writer))
    }

    /**
     * `and` and `or`. C computes the right side after the left one, and only
     * when the left one does not decide the result. A right side that needs
     * statements of its own gets them inside an `if` that tests the left one.
     */
    fn logic(
        &mut self,
        function: &Function<'_>,
        operator: LogicOperator,
        left: &Expr,
        right: &Expr,
        writer: &mut Writer,
    ) -> String {
        let left = self.expr(function, left, writer);
        let mut setup = Writer::at(writer.depth + 1);
        let right = self.expr(function, right, &mut setup);
        let (symbol, needs_right) = match operator {
            LogicOperator::And => ("&&", ""),
            LogicOperator::Or => ("||", "!"),
        };
        if setup.is_empty() {
            return format!("({left} {symbol} {right})");
        }
        let result = self.helper("logic");
        writer.line(&format!("bool {result} = {left};"));
        writer.line(&format!("if ({needs_right}{result}) {{"));
        writer.append(setup);
        writer.line(&format!("{INDENT}{result} = {right};"));
        writer.line("}");
        result
    }

    /// A call. Its arguments are computed from left to right, then the function runs.
    pub(super) fn call(
        &mut self,
        function: &Function<'_>,
        call: &Call,
        writer: &mut Writer,
    ) -> String {
        let last_effect = call
            .arguments
            .iter()
            .rposition(|argument| matches!(argument, Argument::Value(value) if has_effects(value)));
        let arguments: Vec<String> = call
            .arguments
            .iter()
            .enumerate()
            .map(|(position, argument)| match argument {
                Argument::Value(value) => {
                    let code = self.expr(function, value, writer);
                    if last_effect.is_some_and(|last| position < last) && has_effects(value) {
                        self.temporary(value.ty, &code, writer)
                    } else {
                        code
                    }
                }
                Argument::Reference(id) => {
                    let local = &function.locals[*id];
                    let name = c_syntax::local_name(local.name);
                    // A `variable` parameter is already a pointer to the caller's binding.
                    if local.by_reference {
                        name
                    } else {
                        format!("&{name}")
                    }
                }
            })
            .collect();
        let callee = &self.program.functions[call.function];
        format!(
            "{}({})",
            c_syntax::function_name(callee.name),
            arguments.join(", ")
        )
    }

    /**
     * A conversion to `target.ty`. An unchecked one is a plain cast. A checked
     * one first tests the value against the part of the target's range that
     * the source type can reach, in the source's signedness, so no comparison
     * mixes signed and unsigned values.
     */
    fn convert(
        &mut self,
        function: &Function<'_>,
        operand: &Expr,
        target: &Expr,
        checked: bool,
        writer: &mut Writer,
    ) -> String {
        let target_type = c_syntax::type_name(target.ty);
        let value = self.expr(function, operand, writer);
        let (Type::Integer(source), Type::Integer(destination)) = (operand.ty, target.ty) else {
            return format!("(({target_type})({value}))");
        };
        if !checked {
            return format!("(({target_type})({value}))");
        }
        let site = self.site(target.span);
        let maximum = destination.maximum().min(source.maximum());
        if source.is_signed() {
            let minimum = destination.minimum().max(source.minimum());
            format!(
                "(({target_type})lymra_check_signed((int64_t)({value}), {}, {}, {site}))",
                c_syntax::integer_literal(minimum, IntegerType::Integer64),
                c_syntax::integer_literal(maximum, IntegerType::Integer64)
            )
        } else {
            format!(
                "(({target_type})lymra_check_unsigned((uint64_t)({value}), {}, {site}))",
                c_syntax::integer_literal(maximum, IntegerType::Unsigned64)
            )
        }
    }
}
