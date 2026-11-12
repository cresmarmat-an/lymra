/*!
 * C for expressions, computed from left to right.
 *
 * C leaves the order of most operands open: the arguments of a call, and the
 * two sides of `+` or `<`. Lymra runs the parts of an expression from left to
 * right (spec section 4.1), so wherever the order could show, the part that
 * comes first is computed into a temporary before the expression, as a
 * statement of its own. Only parts with effects need it: no part of an
 * expression can change a binding another part reads (`L0508`), so the order
 * of the others never shows.
 */

use super::types::fill_function;
use super::{Generator, INDENT, Writer, c_syntax, local_access};
use crate::ir::{
    Argument, ArithmeticOperator, Call, Expr, ExprKind, Function, IntegerType, LogicOperator, Type,
};
use crate::source::Span;

/// What an index is checked against.
#[derive(Clone, Copy)]
pub(super) struct Bounds {
    /// The length of the array.
    pub length: u64,
    /// Whether the index can fall outside the array.
    pub checked: bool,
    /// Where a failed check points.
    pub span: Span,
}

/// Whether `expr` stands for storage C can take the address of: a local, or an element inside one.
pub(super) fn is_addressable(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Local(_) => true,
        ExprKind::Index { array, .. } => is_addressable(array),
        _ => false,
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
                    return format!("(({})(-{operand}))", self.types.name(&expr.ty));
                }
                let site = self.site(expr.span);
                format!("lymra_negate_{}({operand}, {site})", expr.ty)
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
                        self.types.name(&expr.ty),
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
                format!("lymra_{operation}_{}({left}, {right}, {site})", expr.ty)
            }
            ExprKind::Compare { .. } => format!("({})", self.condition(function, expr, writer)),
            ExprKind::Wrapping {
                operator,
                left,
                right,
            } => {
                let (left, right) = self.pair(function, left, right, writer);
                let ty = self.types.name(&expr.ty);
                let bits = expr.ty.as_integer().map_or(64, IntegerType::bits);
                /*
                 * Signed overflow is undefined in C, so the operation runs in
                 * `uint64_t`, which wraps, and the low bits are read back as
                 * the type. C compilers turn this into one instruction.
                 */
                format!(
                    "(({ty})(uint{bits}_t)((uint64_t)({left}) {} (uint64_t)({right})))",
                    operator.arithmetic().symbol()
                )
            }
            ExprKind::Logic {
                operator,
                left,
                right,
            } => self.logic(function, *operator, left, right, writer),
            ExprKind::Call(call) => self.call(function, call, writer),
            ExprKind::Convert { operand, checked } => {
                self.convert(function, operand, expr, *checked, writer)
            }
            ExprKind::Index {
                array,
                index,
                length,
                checked,
            } => {
                let bounds = Bounds {
                    length: *length,
                    checked: *checked,
                    span: expr.span,
                };
                self.element(function, array, index, bounds, writer)
            }
            ExprKind::Fill(value) => {
                let array = self.types.name(&expr.ty);
                let value = self.expr(function, value, writer);
                format!("{}({value})", fill_function(&array))
            }
            ExprKind::Length(operand) => {
                let code = self.expr(function, operand, writer);
                match &operand.ty {
                    // The array is computed for its effects; its length is part of its type.
                    Type::Array(array) => format!("((void)({code}), INT64_C({}))", array.length),
                    Type::Integer(_) | Type::Boolean | Type::String => {
                        format!("((int64_t)({code}).length)")
                    }
                }
            }
        }
    }

    /// `array[index]`: the array, then the index and its check, in that order.
    fn element(
        &mut self,
        function: &Function<'_>,
        array: &Expr,
        index: &Expr,
        bounds: Bounds,
        writer: &mut Writer,
    ) -> String {
        let code = self.expr(function, array, writer);
        let code = if !is_addressable(array) {
            // An array that is not stored anywhere, such as a call's result, is stored first: C cannot index it in place.
            self.temporary(&array.ty, &code, writer)
        } else if array.has_effects() && (bounds.checked || index.has_effects()) {
            // Only where the array is stored has to be worked out first; its elements are not copied.
            format!("(*{})", self.address(&array.ty, &code, writer))
        } else {
            code
        };
        let position = self.position(function, index, bounds, writer);
        format!("{code}.elements[{position}]")
    }

    /// Works out the address of `code`, stored data of type `ty`, into a new constant and returns its name.
    fn address(&mut self, ty: &Type, code: &str, writer: &mut Writer) -> String {
        let name = self.helper("address");
        let ty = self.types.name(ty);
        writer.line(&format!("const {ty} *{name} = &({code});"));
        name
    }

    /// The position `index` selects in an array. A checked index stops the program unless it lies inside the array.
    pub(super) fn position(
        &mut self,
        function: &Function<'_>,
        index: &Expr,
        bounds: Bounds,
        writer: &mut Writer,
    ) -> String {
        let code = self.expr(function, index, writer);
        if !bounds.checked {
            // The index is known, or proven, to lie inside the array.
            return format!("(size_t)({code})");
        }
        let site = self.site(bounds.span);
        let length = bounds.length;
        match index.ty.as_integer() {
            Some(integer) if integer.is_signed() => {
                format!("lymra_index_signed((int64_t)({code}), UINT64_C({length}), {site})")
            }
            _ => format!("lymra_index_unsigned((uint64_t)({code}), UINT64_C({length}), {site})"),
        }
    }

    /// The address of an array argument: where it is stored, or else a temporary that holds it.
    fn array_address(
        &mut self,
        function: &Function<'_>,
        array: &Expr,
        hoist: bool,
        writer: &mut Writer,
    ) -> String {
        let code = self.expr(function, array, writer);
        if !is_addressable(array) {
            return format!("&{}", self.temporary(&array.ty, &code, writer));
        }
        if hoist {
            // The address is worked out now, with its checks, but the array is not copied.
            self.address(&array.ty, &code, writer)
        } else {
            format!("&({code})")
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
        let is_text = matches!(left.ty, Type::String);
        let (left, right) = self.pair(function, left, right, writer);
        if is_text {
            return format!(
                "lymra_compare_text({left}, {right}) {} 0",
                operator.symbol()
            );
        }
        format!("{left} {} {right}", operator.symbol())
    }

    /// Computes `code`, of type `ty`, into a new constant and returns the constant's name.
    pub(super) fn temporary(&mut self, ty: &Type, code: &str, writer: &mut Writer) -> String {
        self.frame = self.frame.saturating_add(super::array_bytes(ty));
        let name = self.helper("value");
        let ty = self.types.name(ty);
        writer.line(&format!("const {ty} {name} = {code};"));
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
        let left_code = if left.has_effects() && right.has_effects() {
            self.temporary(&left.ty, &left_code, writer)
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
        let last_effect = call.arguments.iter().rposition(
            |argument| matches!(argument, Argument::Value(value) if value.has_effects()),
        );
        let arguments: Vec<String> = call
            .arguments
            .iter()
            .enumerate()
            .map(|(position, argument)| match argument {
                Argument::Value(value) => {
                    let hoist =
                        last_effect.is_some_and(|last| position < last) && value.has_effects();
                    if matches!(value.ty, Type::Array(_)) {
                        return self.array_address(function, value, hoist, writer);
                    }
                    let code = self.expr(function, value, writer);
                    if hoist {
                        self.temporary(&value.ty, &code, writer)
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
        let target_type = self.types.name(&target.ty);
        let value = self.expr(function, operand, writer);
        let (Some(source), Some(destination)) = (operand.ty.as_integer(), target.ty.as_integer())
        else {
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
