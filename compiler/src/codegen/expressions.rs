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

use super::text_types::{TextWalk, text_function};
use super::types::{fill_function, list_function};
use super::{
    Generator, INDENT, Release, Storage, Writer, c_syntax, data_stack, local_access, releases_of,
};
use crate::ir::{
    Argument, ArithmeticOperator, Call, ComparisonOperator, Expr, ExprKind, Extent, FieldValue,
    FloatType, Function, IntegerType, LogicOperator, PrintPart, Sequence, TextForm, Type, Value,
};
use crate::source::Span;

/// What an index is checked against.
#[derive(Clone, Copy)]
pub(super) struct Bounds {
    /// The length of the array, list or view.
    pub length: Extent,
    /// Whether the index can fall outside the array.
    pub checked: bool,
    /// Where a failed check points.
    pub span: Span,
    /// What the error calls the value indexed: "an array", "a list", "a stack" or "a view".
    pub indexed: &'static str,
}

/// What an error calls a value that is indexed or sliced.
pub(super) fn indexed_noun(ty: &Type) -> &'static str {
    match ty {
        Type::List(list) => match list.kind {
            Sequence::List => "a list",
            Sequence::Stack => "a stack",
        },
        Type::View(_) => "a view",
        Type::String => "a text",
        // Only arrays, lists, views and text are indexed or sliced; the checker rejects anything else.
        Type::Array(_)
        | Type::Integer(_)
        | Type::Float(_)
        | Type::Boolean
        | Type::Custom(_)
        | Type::Choice(_)
        | Type::Pool(_)
        | Type::Handle(_) => "an array",
    }
}

/// Whether `expr` stands for storage C can take the address of: a local, or an element or a field inside one.
pub(super) fn is_addressable(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Local(_) => true,
        ExprKind::Index { array: object, .. } | ExprKind::Field { object, .. } => {
            is_addressable(object)
        }
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
                    // A float never fails; for an integer, the optimizer proved it is not the type's lowest value.
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
                let constant = ConstantOperand::of(*operator, left, right);
                let (left, right) = self.pair(function, left, right, writer);
                if !checked {
                    /*
                     * Float arithmetic never fails. For integers, the optimizer
                     * proved the result fits the type and no divisor is zero; C
                     * computes small types in `int`, which holds every value of
                     * them, so the cast back loses nothing.
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
                let ty = &expr.ty;
                match constant {
                    Some(ConstantOperand::Right) => {
                        format!("lymra_{operation}_constant_{ty}({left}, {right}, {site})")
                    }
                    // A constant has no effects, so it can move to the second place of an operation that commutes.
                    Some(ConstantOperand::Left) => {
                        format!("lymra_{operation}_constant_{ty}({right}, {left}, {site})")
                    }
                    None => format!("lymra_{operation}_{ty}({left}, {right}, {site})"),
                }
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
            ExprKind::EmptyList | ExprKind::Text(_) => {
                self.stored_temporary(function, expr, writer)
            }
            ExprKind::Call(_) if Storage::of(&expr.ty) != Storage::None => {
                self.stored_temporary(function, expr, writer)
            }
            ExprKind::Call(call) => self.call(function, call, &[], writer),
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
                    indexed: indexed_noun(&array.ty),
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
                    Type::Integer(_)
                    | Type::Float(_)
                    | Type::Boolean
                    | Type::String
                    | Type::Custom(_)
                    | Type::Choice(_)
                    | Type::List(_)
                    | Type::View(_)
                    | Type::Pool(_)
                    | Type::Handle(_) => format!("((int64_t)({code}).length)"),
                }
            }
            ExprKind::View(items) => self.whole_view(function, items, &expr.ty, writer),
            ExprKind::Pop(stack) => {
                let site = self.site(expr.span);
                let stack_type = self.types.name(&function.locals[*stack].ty);
                // The text of the element popped is copied onto a data stack of the statement's own first.
                let keep = if expr.ty.holds_text() {
                    let (stacks, start) = self.new_stacks(Storage::Text, writer);
                    let keep = data_stack(stacks[0]);
                    self.pending
                        .extend(releases_of(Storage::Text, &stacks, "", start));
                    format!(", {keep}")
                } else {
                    String::new()
                };
                format!(
                    "{}({}{keep}, {site})",
                    list_function(&stack_type, "pop"),
                    super::local_address(function, *stack)
                )
            }
            ExprKind::Top(stack) => {
                let code = self.expr(function, stack, writer);
                let stored = if is_addressable(stack) {
                    code
                } else {
                    self.temporary(&stack.ty, &code, writer)
                };
                let site = self.site(expr.span);
                let stack_type = self.types.name(&stack.ty);
                format!("{}(&({stored}), {site})", list_function(&stack_type, "top"))
            }
            ExprKind::Insert { pool, value } => {
                let value = self.expr(function, value, writer);
                let site = self.site(expr.span);
                let pool_type = self.types.name(&function.locals[*pool].ty);
                format!(
                    "{}({}, {value}, {site})",
                    list_function(&pool_type, "insert"),
                    super::local_address(function, *pool)
                )
            }
            ExprKind::Slot { pool, handle } => {
                let slot = self.slot(function, pool, handle, expr.span, writer);
                format!("{slot}->value")
            }
            ExprKind::Slice { items, start, end } => {
                self.slice(function, items, start, end, expr, writer)
            }
            ExprKind::Bytes(text) => {
                let code = self.expr(function, text, writer);
                let text = if text.has_effects() {
                    self.temporary(&text.ty, &code, writer)
                } else {
                    code
                };
                let view = self.types.name(&expr.ty);
                format!("(({view}){{ (const uint8_t *)({text}).data, (int64_t)({text}).length }})")
            }
            ExprKind::Construct(fields) => self.construct(function, fields, &expr.ty, writer),
            ExprKind::Variant { variant, fields } => {
                self.variant(function, *variant, fields, &expr.ty, writer)
            }
            ExprKind::Field { object, field } => {
                let object = self.expr(function, object, writer);
                format!("({object}).{}", c_syntax::field_name(*field))
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
        let position = self.position(function, index, bounds, &code, writer);
        format!("{code}.elements[{position}]")
    }

    /**
     * A view, of type `view_type`, of every element of `items`, an array or
     * a list. An array that is not stored anywhere, such as a call's result,
     * is stored first, so the view has elements to point at; a list that
     * has to be computed is computed once.
     */
    fn whole_view(
        &mut self,
        function: &Function<'_>,
        items: &Expr,
        view_type: &Type,
        writer: &mut Writer,
    ) -> String {
        let code = self.expr(function, items, writer);
        let view = self.types.name(view_type);
        let stored_first = match items.ty.as_array() {
            Some(_) => !is_addressable(items),
            None => items.has_effects(),
        };
        let stored = if stored_first {
            self.temporary(&items.ty, &code, writer)
        } else {
            code
        };
        let length = match items.ty.as_array() {
            Some(array) => format!("INT64_C({})", array.length),
            None => format!("({stored}).length"),
        };
        format!("(({view}){{ ({stored}).elements, {length} }})")
    }

    /**
     * `slice(items, start, end)`: `items`, then `start`, checked, then `end`,
     * checked, in that order. A slice of text is text; of anything else, a
     * view of type `slice.ty`. A slice of text also has to start and end
     * between characters.
     */
    fn slice(
        &mut self,
        function: &Function<'_>,
        items: &Expr,
        start: &Expr,
        end: &Expr,
        slice: &Expr,
        writer: &mut Writer,
    ) -> String {
        let site = self.site(slice.span);
        let indexed = indexed_noun(&items.ty);
        let text = items.ty == Type::String;
        let whole = if text || items.ty.as_view().is_some() {
            self.expr(function, items, writer)
        } else {
            self.whole_view(function, items, &slice.ty, writer)
        };
        let whole = self.temporary(&slice.ty, &whole, writer);
        let length = format!("(uint64_t)({whole}).length");
        let start = self.slice_position(function, start, None, (&length, indexed, &site), writer);
        if text {
            writer.line(&format!(
                "lymra_text_boundary({whole}, {start}, \"start\", {site});"
            ));
        }
        let end = self.slice_position(
            function,
            end,
            Some(&start),
            (&length, indexed, &site),
            writer,
        );
        if text {
            writer.line(&format!(
                "lymra_text_boundary({whole}, {end}, \"end\", {site});"
            ));
            return format!(
                "((lymra_string){{ ({whole}).data + {start}, (size_t)({end} - {start}) }})"
            );
        }
        let view = self.types.name(&slice.ty);
        format!("(({view}){{ ({whole}).elements + {start}, {end} - {start} }})")
    }

    /**
     * Computes one end of a slice into a new `int64_t` and checks it: the
     * start lies from 0 to the length, the end from `start` to the length.
     * `limits` holds the length, what the slice is taken from, and the place
     * a failed check points at.
     */
    fn slice_position(
        &mut self,
        function: &Function<'_>,
        position: &Expr,
        start: Option<&str>,
        limits: (&str, &str, &str),
        writer: &mut Writer,
    ) -> String {
        let (length, indexed, site) = limits;
        let code = self.expr(function, position, writer);
        let signed = position.ty.as_integer().is_some_and(IntegerType::is_signed);
        let (value, form) = if signed {
            (format!("(int64_t)({code})"), "signed")
        } else {
            (format!("(uint64_t)({code})"), "unsigned")
        };
        let check = match start {
            None => format!("lymra_slice_start_{form}({value}, {length}, \"{indexed}\", {site})"),
            Some(start) => {
                format!("lymra_slice_end_{form}({value}, {start}, {length}, \"{indexed}\", {site})")
            }
        };
        let name = self.helper(if start.is_none() { "start" } else { "end" });
        writer.line(&format!("const int64_t {name} = {check};"));
        name
    }

    /**
     * A value that keeps something on data stacks, which a statement computes
     * without a binding: a list, or text. It is built on data stacks of its
     * own, which the statement that uses it frees once it is done.
     */
    pub(super) fn stored_temporary(
        &mut self,
        function: &Function<'_>,
        expr: &Expr,
        writer: &mut Writer,
    ) -> String {
        let storage = Storage::of(&expr.ty);
        let (stacks, text_start) = self.new_stacks(storage, writer);
        let names: Vec<String> = stacks.iter().map(|&stack| data_stack(stack)).collect();
        let code = self.build_value(function, expr, &names, writer);
        let name = self.helper("value");
        let ty = self.types.name(&expr.ty);
        writer.line(&format!("const {ty} {name} = {code};"));
        self.pending
            .extend(releases_of(storage, &stacks, &name, text_start));
        name
    }

    /**
     * The C that builds `value`, which keeps something on data stacks, on top
     * of `stacks`, in the order a function that returns it takes them. A list
     * starts empty, a call builds what it returns there, and text is built
     * from its parts; any other value is copied there from where it is kept,
     * except text the program itself keeps.
     */
    pub(super) fn build_value(
        &mut self,
        function: &Function<'_>,
        value: &Expr,
        stacks: &[String],
        writer: &mut Writer,
    ) -> String {
        let ty = self.types.name(&value.ty);
        let joined = stacks.join(", ");
        match &value.kind {
            ExprKind::EmptyList => format!("{}({joined})", list_function(&ty, "start")),
            ExprKind::Call(call) => self.call(function, call, stacks, writer),
            ExprKind::Text(parts) => self.build_text(function, parts, &joined, value.span, writer),
            _ => {
                let source = self.expr(function, value, writer);
                let site = self.site(value.span);
                if Storage::of(&value.ty) == Storage::Text {
                    let owned = self.helper("owned");
                    writer.line(&format!("{ty} {owned} = {source};"));
                    let own = text_function(&mut self.types, &value.ty, TextWalk::Own);
                    writer.line(&format!("{own}(&{owned}, {joined}, {site});"));
                    owned
                } else {
                    format!(
                        "{}({joined}, {source}, {site})",
                        list_function(&ty, "place")
                    )
                }
            }
        }
    }

    /**
     * The C for text built from `parts` at the top of the data stack
     * `stack`. Every value is computed first, so text that a value builds
     * goes on other stacks, and nothing comes between the parts.
     */
    pub(super) fn build_text(
        &mut self,
        function: &Function<'_>,
        parts: &[PrintPart],
        stack: &str,
        span: Span,
        writer: &mut Writer,
    ) -> String {
        let builder = self.helper("text");
        let mut additions = Vec::new();
        for part in parts {
            match part {
                PrintPart::Text(text) if text.is_empty() => {}
                PrintPart::Text(text) => additions.push(format!(
                    "lymra_text_add_text(&{builder}, {});",
                    c_syntax::text_value(text)
                )),
                PrintPart::Value { value, form } => {
                    self.writes_floats |= matches!(form, TextForm::Float(_));
                    let code = self.expr(function, value, writer);
                    let code = if value.has_effects() {
                        self.temporary(&value.ty, &code, writer)
                    } else {
                        code
                    };
                    additions.push(add_value(*form, &builder, &code));
                }
            }
        }
        let site = self.site(span);
        writer.line(&format!(
            "lymra_text_builder {builder} = lymra_text_begin({stack}, {site});"
        ));
        for addition in additions {
            writer.line(&addition);
        }
        format!("lymra_text_end(&{builder})")
    }

    /// Works out the address of `code`, stored data of type `ty`, into a new constant and returns its name.
    fn address(&mut self, ty: &Type, code: &str, writer: &mut Writer) -> String {
        let name = self.helper("address");
        let ty = self.types.name(ty);
        writer.line(&format!("const {ty} *{name} = &({code});"));
        name
    }

    /**
     * The position `index` selects in an array or a list, whose C is
     * `indexed`. A checked index stops the program unless it lies inside.
     */
    pub(super) fn position(
        &mut self,
        function: &Function<'_>,
        index: &Expr,
        bounds: Bounds,
        indexed: &str,
        writer: &mut Writer,
    ) -> String {
        let code = self.expr(function, index, writer);
        if !bounds.checked {
            // The index is known, or proven, to lie inside the array.
            return format!("(size_t)({code})");
        }
        let site = self.site(bounds.span);
        let length = match bounds.length {
            Extent::Fixed(length) => format!("UINT64_C({length})"),
            Extent::Dynamic => format!("(uint64_t)({indexed}).length"),
        };
        let noun = bounds.indexed;
        match index.ty.as_integer() {
            Some(integer) if integer.is_signed() => {
                format!("lymra_index_signed((int64_t)({code}), {length}, \"{noun}\", {site})")
            }
            _ => format!("lymra_index_unsigned((uint64_t)({code}), {length}, \"{noun}\", {site})"),
        }
    }

    /// A value of a declared type, as a C compound literal.
    fn construct(
        &mut self,
        function: &Function<'_>,
        fields: &[FieldValue],
        ty: &Type,
        writer: &mut Writer,
    ) -> String {
        let fields = self.initializers(function, fields, writer);
        let name = self.types.name(ty);
        format!("(({name}){{ {fields} }})")
    }

    /**
     * A value of a choice, as a C compound literal: its tag, then the
     * fields of its variant, if it has any, in the variant's struct.
     */
    fn variant(
        &mut self,
        function: &Function<'_>,
        variant: usize,
        fields: &[FieldValue],
        ty: &Type,
        writer: &mut Writer,
    ) -> String {
        let name = self.types.name(ty);
        let tag = format!(".{} = {variant}", c_syntax::TAG);
        if fields.is_empty() {
            return format!("(({name}){{ {tag} }})");
        }
        let fields = self.initializers(function, fields, writer);
        format!(
            "(({name}){{ {tag}, .{} = {{ .{} = {{ {fields} }} }} }})",
            c_syntax::VARIANTS,
            c_syntax::variant_name(variant)
        )
    }

    /**
     * The designated initializers of a construction's fields. They are
     * computed in the order they are written, and each one with effects but
     * the last goes into a temporary first, since C computes the values of
     * an initializer in any order.
     */
    fn initializers(
        &mut self,
        function: &Function<'_>,
        fields: &[FieldValue],
        writer: &mut Writer,
    ) -> String {
        let last_effect = fields.iter().rposition(|field| field.value.has_effects());
        let members: Vec<String> = fields
            .iter()
            .enumerate()
            .map(|(position, field)| {
                let code = self.expr(function, &field.value, writer);
                let code = if last_effect.is_some_and(|last| position < last)
                    && field.value.has_effects()
                {
                    self.temporary(&field.value.ty, &code, writer)
                } else {
                    code
                };
                format!(".{} = {code}", c_syntax::field_name(field.field))
            })
            .collect();
        members.join(", ")
    }

    /// The address of an argument that holds other values: where it is stored, or else a temporary that holds it.
    fn compound_address(
        &mut self,
        function: &Function<'_>,
        value: &Expr,
        hoist: bool,
        writer: &mut Writer,
    ) -> String {
        let code = self.expr(function, value, writer);
        if !is_addressable(value) {
            return format!("&{}", self.temporary(&value.ty, &code, writer));
        }
        if hoist {
            // The address is worked out now, with its checks, but the value is not copied.
            self.address(&value.ty, &code, writer)
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
        let operands = &left.ty;
        let (left, right) = self.pair(function, left, right, writer);
        let symbol = operator.symbol();
        match operands {
            Type::String => format!("lymra_compare_text({left}, {right}) {symbol} 0"),
            // The checker lets only `==` and `!=` compare handles.
            Type::Handle(_) => {
                let same = format!("lymra_same_handle({left}, {right})");
                if matches!(operator, ComparisonOperator::Equal) {
                    same
                } else {
                    format!("(!{same})")
                }
            }
            // Only a choice whose variants hold no fields is compared, by its tag alone.
            Type::Choice(_) => format!(
                "({left}).{tag} {symbol} ({right}).{tag}",
                tag = c_syntax::TAG
            ),
            Type::Integer(_)
            | Type::Float(_)
            | Type::Boolean
            | Type::Array(_)
            | Type::Custom(_)
            | Type::List(_)
            | Type::View(_)
            | Type::Pool(_) => {
                format!("{left} {symbol} {right}")
            }
        }
    }

    /**
     * The C for a pointer to the slot of `pool` that `handle` names, whose
     * place in the source is `span`. Finding it stops the program when the
     * handle is stale.
     */
    pub(super) fn slot(
        &mut self,
        function: &Function<'_>,
        pool: &Expr,
        handle: &Expr,
        span: Span,
        writer: &mut Writer,
    ) -> String {
        let (pool_code, handle) = self.pair(function, pool, handle, writer);
        let stored = if is_addressable(pool) {
            pool_code
        } else {
            self.temporary(&pool.ty, &pool_code, writer)
        };
        self.pool_slot(&pool.ty, &stored, &handle, span)
    }

    /// The C that finds the slot `handle` names in the pool `stored`, of type `ty`, which C can take the address of.
    pub(super) fn pool_slot(
        &mut self,
        ty: &Type,
        stored: &str,
        handle: &str,
        span: Span,
    ) -> String {
        let site = self.site(span);
        let pool_type = self.types.name(ty);
        format!(
            "{}(&({stored}), {handle}, {site})",
            list_function(&pool_type, "find")
        )
    }

    /// Computes `code`, of type `ty`, into a new constant and returns the constant's name.
    pub(super) fn temporary(&mut self, ty: &Type, code: &str, writer: &mut Writer) -> String {
        self.frame = self.frame.saturating_add(super::compound_bytes(ty));
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
        let earlier = self.pending.len();
        let right = self.expr(function, right, &mut setup);
        // Lists the right side builds exist only when it runs, so they are freed where it runs.
        let built: Vec<Release> = self.pending.drain(earlier..).collect();
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
        writer.depth += 1;
        Self::release(writer, &built);
        writer.depth -= 1;
        writer.line("}");
        result
    }

    /**
     * A call. Its arguments are computed from left to right, then the function
     * runs. A function that returns a list builds it on `destination`.
     */
    pub(super) fn call(
        &mut self,
        function: &Function<'_>,
        call: &Call,
        destinations: &[String],
        writer: &mut Writer,
    ) -> String {
        let last_effect = call.arguments.iter().rposition(
            |argument| matches!(argument, Argument::Value(value) if value.has_effects()),
        );
        let program = self.program;
        let callee = &program.functions[call.function];
        let mut arguments: Vec<String> = destinations.to_vec();
        for (position, argument) in call.arguments.iter().enumerate() {
            match argument {
                Argument::Value(value) => {
                    let hoist =
                        last_effect.is_some_and(|last| position < last) && value.has_effects();
                    let code = if value.ty.is_compound() {
                        self.compound_address(function, value, hoist, writer)
                    } else {
                        let code = self.expr(function, value, writer);
                        if hoist {
                            self.temporary(&value.ty, &code, writer)
                        } else {
                            code
                        }
                    };
                    arguments.push(code);
                }
                Argument::Reference { local: id, .. } => {
                    let local = &function.locals[*id];
                    let name = c_syntax::local_name(local.name);
                    // A `variable` parameter is already a pointer to the caller's binding.
                    arguments.push(if local.by_reference {
                        name
                    } else {
                        format!("&{name}")
                    });
                    // Text the callee gives the binding goes where the binding keeps its text.
                    let takes_text = callee.parameters.get(position).is_some_and(|&parameter| {
                        Storage::of(&callee.locals[parameter].ty) == Storage::Text
                    });
                    if let (true, Some(region)) = (takes_text, self.text_regions.get(id)) {
                        arguments.push(region.stack.clone());
                        arguments.push(region.start.clone());
                    }
                }
            }
        }
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
        if !checked {
            return format!("(({target_type})({value}))");
        }
        let site = self.site(target.span);
        let (Some(source), Some(destination)) = (operand.ty.as_integer(), target.ty.as_integer())
        else {
            return match target.ty.as_integer() {
                // To an integer: the whole part has to fit, which leaves out NaN.
                Some(destination) => {
                    let (below, above) = float_bounds(destination);
                    format!(
                        "(({target_type})lymra_check_float((double)({value}), {}, {}, {site}))",
                        c_syntax::float_literal(below.to_bits(), FloatType::Float64),
                        c_syntax::float_literal(above.to_bits(), FloatType::Float64)
                    )
                }
                // From `float64` to `float32`: the only float conversion that can fail.
                None => format!("lymra_narrow_float({value}, {site})"),
            };
        };
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

/**
 * The floats just outside an integer type: a float strictly between them
 * keeps its whole part inside the type, and every other float, NaN too, does
 * not. Below the lowest value of a signed 64-bit type, the next float down is
 * 2048 lower; every other bound is exact.
 */
fn float_bounds(ty: IntegerType) -> (f64, f64) {
    let above = 2.0_f64.powi(ty.bits() as i32 - i32::from(ty.is_signed()));
    let below = match (ty.is_signed(), ty.bits()) {
        (false, _) => -1.0,
        (true, 64) => -(2.0_f64.powi(63)) - 2048.0,
        (true, _) => ty.minimum() as f64 - 1.0,
    };
    (below, above)
}

/**
 * Which operand of a checked `+`, `-` or `*` is a constant of the program,
 * whose check then compares the other operand with limits the C compiler
 * works out (see `runtime.h`). Both operands are never constants: the checker
 * computes such an operation itself.
 */
#[derive(Clone, Copy)]
enum ConstantOperand {
    Left,
    Right,
}

impl ConstantOperand {
    fn of(operator: ArithmeticOperator, left: &Expr, right: &Expr) -> Option<Self> {
        let commutes = match operator {
            ArithmeticOperator::Add | ArithmeticOperator::Multiply => true,
            ArithmeticOperator::Subtract => false,
            ArithmeticOperator::Divide | ArithmeticOperator::Remainder => return None,
        };
        if is_constant(right) {
            Some(ConstantOperand::Right)
        } else if commutes && is_constant(left) {
            Some(ConstantOperand::Left)
        } else {
            None
        }
    }
}

/// Whether an integer operand is known while compiling: a literal, or a named constant.
fn is_constant(operand: &Expr) -> bool {
    matches!(
        operand.kind,
        ExprKind::Literal(Value::Integer { .. }) | ExprKind::Constant(_)
    )
}

/// The C statement that adds `value`, written as `form` says, to the text `builder` builds.
fn add_value(form: TextForm, builder: &str, value: &str) -> String {
    match form {
        TextForm::Text => format!("lymra_text_add_text(&{builder}, {value});"),
        TextForm::Boolean => format!("lymra_text_add_boolean(&{builder}, {value});"),
        TextForm::Signed => format!("lymra_text_add_signed(&{builder}, (int64_t)({value}));"),
        TextForm::Unsigned => format!("lymra_text_add_unsigned(&{builder}, (uint64_t)({value}));"),
        TextForm::Float(FloatType::Float32) => {
            format!("lymra_text_add_float32(&{builder}, {value});")
        }
        TextForm::Float(FloatType::Float64) => {
            format!("lymra_text_add_float64(&{builder}, {value});")
        }
    }
}
