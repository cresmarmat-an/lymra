/*!
 * Runs a checked program exactly, with every integer as an `i128`, and stops
 * at the first operation that fails without a check.
 */

use std::fmt::Write as _;

use lymra::ir::{
    Argument, ArithmeticOperator, Attempt, Block, Call, ComparisonOperator, Expr, ExprKind,
    FAILURE_VARIANT, FieldValue, FunctionId, IntegerType, LocalId, LogicOperator, PlaceStep,
    PrintPart, Program, SUCCESS_VARIANT, Statement, Type, Value,
};
use lymra::source::Span;

/// Statements and loop iterations one run may take before it is stopped.
const STEP_BUDGET: usize = 5_000;

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    Finished,
    /// A runtime check stopped the program, as it would stop the compiled one.
    Trapped,
    OutOfSteps,
}

/// How a run ended and what it printed before that.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ending: Ending,
    pub output: String,
}

/**
 * Calls `function` with integer arguments and runs it to the end. Fails with
 * a description when an operation without a check overflows or divides by
 * zero, which means the check should not have been removed.
 */
pub fn run(
    program: &Program<'_>,
    function: FunctionId,
    arguments: &[i128],
) -> Result<Outcome, String> {
    let mut interpreter = Interpreter {
        program,
        steps: 0,
        output: String::new(),
    };
    let arguments = arguments
        .iter()
        .map(|&value| Slot::Integer(value))
        .collect();
    let ending = match interpreter.call(function, arguments) {
        Ok(_) => Ending::Finished,
        Err(Stop::Ended(ending)) => ending,
        Err(Stop::Unsound(problem)) => return Err(problem),
        // A function catches the failures of its own `try`s and returns them.
        Err(Stop::Failed(result)) => {
            return Err(format!("a failure {result:?} left a function"));
        }
    };
    Ok(Outcome {
        ending,
        output: interpreter.output,
    })
}

#[derive(Clone, Debug)]
enum Slot {
    Integer(i128),
    /// The generator writes no floats, so only values and comparisons of them are run.
    Float(f64),
    Boolean(bool),
    Text(String),
    /**
     * The elements of an array or a list, or the fields of a value of a
     * declared type, in order. A pool is the list of its slots, each its
     * generation and its value; a handle is its slot and generation.
     */
    Compound(Vec<Slot>),
    /// A value of a choice: the position of its variant, and the variant's fields in order.
    Variant {
        variant: usize,
        fields: Vec<Slot>,
    },
    /// A local before its declaration runs, or the result of a function that returns nothing.
    Unset,
}

/// What a `for` loop over an array reads its elements from.
enum Elements {
    /// A local, or an element inside it, at these positions; read as the loop reaches each element.
    Stored {
        local: LocalId,
        positions: Vec<usize>,
    },
    /// A value computed once.
    Computed(Vec<Slot>),
}

/// Why evaluation stopped before the end of the function.
enum Stop {
    Ended(Ending),
    /// An operation without a check failed.
    Unsound(String),
    /// A `try` whose call failed ends the function it is in with this result.
    Failed(Slot),
}

type Eval<T> = Result<T, Stop>;

/// Where control goes after a statement.
enum Flow {
    Next,
    Break,
    Continue,
    Return(Option<Slot>),
}

struct Interpreter<'program, 'src> {
    program: &'program Program<'src>,
    steps: usize,
    output: String,
}

impl Interpreter<'_, '_> {
    fn step(&mut self) -> Eval<()> {
        self.steps += 1;
        if self.steps > STEP_BUDGET {
            return Err(Stop::Ended(Ending::OutOfSteps));
        }
        Ok(())
    }

    /// Runs a function. Returns its result and the final values of its parameters.
    fn call(
        &mut self,
        function: FunctionId,
        arguments: Vec<Slot>,
    ) -> Eval<(Option<Slot>, Vec<Slot>)> {
        let program = self.program;
        let definition = &program.functions[function];
        let mut slots = vec![Slot::Unset; definition.locals.len()];
        for (&parameter, argument) in definition.parameters.iter().zip(arguments) {
            slots[parameter] = argument;
        }
        let result = match self.block(&definition.body, &mut slots) {
            Ok(Flow::Return(value)) => value,
            Ok(Flow::Next | Flow::Break | Flow::Continue) => None,
            Err(Stop::Failed(result)) => Some(result),
            Err(stop) => return Err(stop),
        };
        let parameters = definition
            .parameters
            .iter()
            .map(|&parameter| slots[parameter].clone())
            .collect();
        Ok((result, parameters))
    }

    /**
     * Calls a function from inside the program. A `variable` argument is
     * copied in and its final value copied back, which matches passing it by
     * reference because rule L0508 keeps the caller from using it anywhere
     * else in the same expression.
     */
    fn call_expr(&mut self, call: &Call, slots: &mut [Slot]) -> Eval<Option<Slot>> {
        let mut arguments = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            arguments.push(match argument {
                Argument::Value(value) => self.expr(value, slots)?,
                Argument::Reference { local, .. } => slots[*local].clone(),
            });
        }
        let (result, parameters) = self.call(call.function, arguments)?;
        for (argument, value) in call.arguments.iter().zip(parameters) {
            if let Argument::Reference { local, .. } = argument {
                slots[*local] = value;
            }
        }
        Ok(result)
    }

    fn block(&mut self, block: &Block, slots: &mut [Slot]) -> Eval<Flow> {
        for statement in &block.statements {
            let flow = self.statement(statement, slots)?;
            if !matches!(flow, Flow::Next) {
                return Ok(flow);
            }
        }
        Ok(Flow::Next)
    }

    fn statement(&mut self, statement: &Statement, slots: &mut [Slot]) -> Eval<Flow> {
        self.step()?;
        match statement {
            Statement::Declare { local, value } => {
                slots[*local] = self.expr(value, slots)?;
            }
            Statement::Assign { target, value } => {
                let positions = self.positions(target.local, &target.steps, slots)?;
                let value = self.expr(value, slots)?;
                *element_mut(&mut slots[target.local], &positions)? = value;
            }
            Statement::Push { list, value, .. } => {
                let value = self.expr(value, slots)?;
                match &mut slots[*list] {
                    Slot::Compound(elements) => elements.push(value),
                    other => return Err(Stop::Unsound(format!("a push onto {other:?}"))),
                }
            }
            Statement::Remove { pool, handle, .. } => {
                let handle = self.expr(handle, slots)?;
                let slot = found_slot(&mut slots[*pool], &handle)?;
                if let Slot::Compound(entry) = slot {
                    entry[0] = match entry[0] {
                        Slot::Integer(generation) => Slot::Integer(generation + 1),
                        ref other => return Err(Stop::Unsound(format!("a generation {other:?}"))),
                    };
                }
            }
            Statement::Discard(value) => {
                self.expr(value, slots)?;
            }
            Statement::Call(call) => {
                self.call_expr(call, slots)?;
            }
            Statement::Print(parts) => {
                // Every value is computed before the line starts, as in the C the compiler makes.
                let line = self.text(parts, slots)?;
                self.output.push_str(&line);
                self.output.push('\n');
            }
            Statement::If {
                branches,
                otherwise,
            } => {
                for branch in branches {
                    if self.condition(&branch.condition, slots)? {
                        return self.block(&branch.body, slots);
                    }
                }
                if let Some(block) = otherwise {
                    return self.block(block, slots);
                }
            }
            Statement::While { condition, body } => {
                while self.condition(condition, slots)? {
                    self.step()?;
                    match self.block(body, slots)? {
                        Flow::Next | Flow::Continue => {}
                        Flow::Break => break,
                        flow @ Flow::Return(_) => return Ok(flow),
                    }
                }
            }
            Statement::ForRange {
                variable,
                start,
                end,
                body,
            } => {
                let start = self.integer(start, slots)?;
                let end = self.integer(end, slots)?;
                for value in start..end {
                    self.step()?;
                    if let Some(variable) = variable {
                        slots[*variable] = Slot::Integer(value);
                    }
                    match self.block(body, slots)? {
                        Flow::Next | Flow::Continue => {}
                        Flow::Break => break,
                        flow @ Flow::Return(_) => return Ok(flow),
                    }
                }
            }
            Statement::ForEach {
                variable,
                array,
                body,
                ..
            } => {
                let elements = self.elements(array, slots)?;
                // The loop goes over the elements there are when it starts, as the C does.
                let count = match &elements {
                    Elements::Stored { local, positions } => {
                        compound_length(element_mut(&mut slots[*local], positions)?)?
                    }
                    Elements::Computed(values) => values.len(),
                };
                for position in 0..count {
                    self.step()?;
                    let element = match &elements {
                        Elements::Stored { local, positions } => {
                            let mut path = positions.clone();
                            path.push(position);
                            element_mut(&mut slots[*local], &path)?.clone()
                        }
                        Elements::Computed(values) => {
                            values.get(position).cloned().ok_or_else(|| {
                                Stop::Unsound("an array shorter than its type".to_owned())
                            })?
                        }
                    };
                    if let Some(variable) = variable {
                        slots[*variable] = element;
                    }
                    match self.block(body, slots)? {
                        Flow::Next | Flow::Continue => {}
                        Flow::Break => break,
                        flow @ Flow::Return(_) => return Ok(flow),
                    }
                }
            }
            Statement::Match { value, cases } => {
                let Slot::Variant { variant, fields } = self.expr(value, slots)? else {
                    return Err(Stop::Unsound("a match on a value of no choice".to_owned()));
                };
                let case = cases
                    .iter()
                    .find(|case| case.variant == variant)
                    .ok_or_else(|| Stop::Unsound(format!("no case for variant {variant}")))?;
                for binding in &case.bindings {
                    slots[binding.local] = fields.get(binding.field).cloned().ok_or_else(|| {
                        Stop::Unsound(format!("field {} past the end", binding.field))
                    })?;
                }
                return self.block(&case.body, slots);
            }
            Statement::Return(value) => {
                let value = match value {
                    Some(value) => Some(self.expr(value, slots)?),
                    None => None,
                };
                return Ok(Flow::Return(value));
            }
            Statement::Try(attempt) => {
                self.attempt(attempt, slots)?;
            }
            Statement::Break => return Ok(Flow::Break),
            Statement::Continue => return Ok(Flow::Continue),
            Statement::Assert { condition } => {
                if !self.condition(condition, slots)? {
                    return Err(Stop::Ended(Ending::Trapped));
                }
            }
        }
        Ok(Flow::Next)
    }

    /// `try call`: the value of a `Success`, or a stop that fails the function with the error or the fallback.
    fn attempt(&mut self, attempt: &Attempt, slots: &mut [Slot]) -> Eval<Option<Slot>> {
        match self.expr(&attempt.result, slots)? {
            Slot::Variant {
                variant: SUCCESS_VARIANT,
                fields,
            } => Ok(fields.into_iter().next()),
            Slot::Variant {
                variant: FAILURE_VARIANT,
                fields,
            } => {
                let error = match &attempt.fallback {
                    Some(fallback) => self.expr(fallback, slots)?,
                    None => fields
                        .into_iter()
                        .next()
                        .ok_or_else(|| Stop::Unsound("a failure without its error".to_owned()))?,
                };
                Err(Stop::Failed(Slot::Variant {
                    variant: FAILURE_VARIANT,
                    fields: vec![error],
                }))
            }
            other => Err(Stop::Unsound(format!("a `try` on {other:?}"))),
        }
    }

    /// Computes and checks the steps of an assignment's target, from `local` inward, in order.
    fn positions(
        &mut self,
        local: LocalId,
        steps: &[PlaceStep],
        slots: &mut [Slot],
    ) -> Eval<Vec<usize>> {
        let mut positions = Vec::with_capacity(steps.len());
        for step in steps {
            let position = match step {
                PlaceStep::Index(index) => {
                    let value = self.integer(&index.value, slots)?;
                    let length = compound_length(element_mut(&mut slots[local], &positions)?)?;
                    check_index(value, length, index.checked, index.span)?
                }
                PlaceStep::Field(field) => *field,
                PlaceStep::Slot(slot) => {
                    let handle = self.expr(&slot.handle, slots)?;
                    let pool = element_mut(&mut slots[local], &positions)?;
                    found_slot(pool, &handle)?;
                    positions.push(slot_of(&handle)?);
                    // The value comes after the slot's generation.
                    1
                }
            };
            positions.push(position);
        }
        Ok(positions)
    }

    /// Where a `for` loop over `array` reads its elements: storage when `array` names some, as the C does.
    fn elements(&mut self, array: &Expr, slots: &mut [Slot]) -> Eval<Elements> {
        match stored_path(array) {
            Some((local, steps)) => {
                let mut positions = Vec::with_capacity(steps.len());
                for step in steps {
                    let position = match step {
                        Stored::Index(index, checked, span) => {
                            let value = self.integer(index, slots)?;
                            let length =
                                compound_length(element_mut(&mut slots[local], &positions)?)?;
                            check_index(value, length, checked, span)?
                        }
                        Stored::Field(field) => field,
                    };
                    positions.push(position);
                }
                Ok(Elements::Stored { local, positions })
            }
            None => match self.expr(array, slots)? {
                Slot::Compound(values) => Ok(Elements::Computed(values)),
                other => Err(Stop::Unsound(format!("a loop over {other:?}"))),
            },
        }
    }

    /// The values of a construction's `count` fields, computed in the order they are written.
    fn fields(
        &mut self,
        fields: &[FieldValue],
        count: usize,
        slots: &mut [Slot],
    ) -> Eval<Vec<Slot>> {
        let mut values = vec![Slot::Unset; count];
        for field in fields {
            let value = self.expr(&field.value, slots)?;
            let slot = values
                .get_mut(field.field)
                .ok_or_else(|| Stop::Unsound(format!("field {} past the end", field.field)))?;
            *slot = value;
        }
        Ok(values)
    }

    /**
     * `slice(items, start, end)`: the start is computed and checked before
     * the end, as in the compiled program, and a slice of text has to start
     * and end between characters.
     */
    fn slice(&mut self, items: &Expr, start: &Expr, end: &Expr, slots: &mut [Slot]) -> Eval<Slot> {
        let items = self.expr(items, slots)?;
        let length = match &items {
            Slot::Compound(values) => values.len(),
            Slot::Text(text) => text.len(),
            other => return Err(Stop::Unsound(format!("a slice of {other:?}"))),
        };
        let start = self.integer(start, slots)?;
        let start = slice_position(start, 0, length)?;
        if let Slot::Text(text) = &items {
            if !text.is_char_boundary(start) {
                return Err(Stop::Ended(Ending::Trapped));
            }
        }
        let end = self.integer(end, slots)?;
        let end = slice_position(end, start, length)?;
        match items {
            Slot::Compound(values) => Ok(Slot::Compound(values[start..end].to_vec())),
            Slot::Text(text) => text
                .get(start..end)
                .map(|part| Slot::Text(part.to_owned()))
                .ok_or(Stop::Ended(Ending::Trapped)),
            other => Err(Stop::Unsound(format!("a slice of {other:?}"))),
        }
    }

    /// The text that `parts` make, each value written as `print` writes it.
    fn text(&mut self, parts: &[PrintPart], slots: &mut [Slot]) -> Eval<String> {
        let mut text = String::new();
        for part in parts {
            match part {
                PrintPart::Text(literal) => text.push_str(literal),
                PrintPart::Value { value, .. } => {
                    let value = self.expr(value, slots)?;
                    let _ = write!(text, "{}", describe(&value));
                }
            }
        }
        Ok(text)
    }

    fn condition(&mut self, expr: &Expr, slots: &mut [Slot]) -> Eval<bool> {
        Ok(matches!(self.expr(expr, slots)?, Slot::Boolean(true)))
    }

    fn integer(&mut self, expr: &Expr, slots: &mut [Slot]) -> Eval<i128> {
        match self.expr(expr, slots)? {
            Slot::Integer(value) => Ok(value),
            other => Err(Stop::Unsound(format!(
                "expected an integer, found {other:?}"
            ))),
        }
    }

    fn expr(&mut self, expr: &Expr, slots: &mut [Slot]) -> Eval<Slot> {
        let ty = expr.ty.as_integer();
        let (result, checked, what) = match &expr.kind {
            ExprKind::Literal(value) => return Ok(from_value(value)),
            ExprKind::Local(local) => return Ok(slots[*local].clone()),
            ExprKind::Constant(id) => return Ok(from_value(&self.program.constants[*id].value)),
            ExprKind::Not(operand) => return Ok(Slot::Boolean(!self.condition(operand, slots)?)),
            ExprKind::Compare {
                operator,
                left,
                right,
            } => {
                let left = self.expr(left, slots)?;
                let right = self.expr(right, slots)?;
                return Ok(Slot::Boolean(compare(*operator, &left, &right)));
            }
            ExprKind::Logic {
                operator,
                left,
                right,
            } => {
                let left = self.condition(left, slots)?;
                let decided = match operator {
                    LogicOperator::And => !left,
                    LogicOperator::Or => left,
                };
                if decided {
                    return Ok(Slot::Boolean(left));
                }
                return Ok(Slot::Boolean(self.condition(right, slots)?));
            }
            ExprKind::Call(call) => return Ok(self.call_expr(call, slots)?.unwrap_or(Slot::Unset)),
            ExprKind::Try(attempt) => {
                return self
                    .attempt(attempt, slots)?
                    .ok_or_else(|| Stop::Unsound("a `try` without a value".to_owned()));
            }
            ExprKind::Index {
                array,
                index,
                checked,
                ..
            } => {
                let array = self.expr(array, slots)?;
                let index = self.integer(index, slots)?;
                let Slot::Compound(mut values) = array else {
                    return Err(Stop::Unsound(format!("an element of {array:?}")));
                };
                let position = check_index(index, values.len(), *checked, expr.span)?;
                return Ok(values.swap_remove(position));
            }
            ExprKind::Wrapping {
                operator,
                left,
                right,
            } => {
                let left = self.integer(left, slots)?;
                let right = self.integer(right, slots)?;
                let ty =
                    ty.ok_or_else(|| Stop::Unsound("wrapping without an integer type".to_owned()))?;
                return Ok(Slot::Integer(operator.apply(left, right, ty)));
            }
            ExprKind::Fill(value) => {
                let value = self.expr(value, slots)?;
                let length = expr
                    .ty
                    .as_array()
                    .and_then(|array| usize::try_from(array.length).ok())
                    .ok_or_else(|| Stop::Unsound("a fill without an array type".to_owned()))?;
                return Ok(Slot::Compound(vec![value; length]));
            }
            ExprKind::EmptyList => return Ok(Slot::Compound(Vec::new())),
            ExprKind::Text(parts) => return Ok(Slot::Text(self.text(parts, slots)?)),
            ExprKind::Pop(stack) => {
                return match &mut slots[*stack] {
                    Slot::Compound(elements) => elements.pop().ok_or(Stop::Ended(Ending::Trapped)),
                    other => Err(Stop::Unsound(format!("a pop from {other:?}"))),
                };
            }
            ExprKind::Insert { pool, value } => {
                let value = self.expr(value, slots)?;
                return match &mut slots[*pool] {
                    Slot::Compound(entries) => {
                        // Slots are never reused here, so every value starts the first generation of its own.
                        let slot = entries.len() as i128;
                        entries.push(Slot::Compound(vec![Slot::Integer(0), value]));
                        Ok(Slot::Compound(vec![Slot::Integer(slot), Slot::Integer(0)]))
                    }
                    other => Err(Stop::Unsound(format!("an insert into {other:?}"))),
                };
            }
            ExprKind::Slot { pool, handle } => {
                let mut pool = self.expr(pool, slots)?;
                let handle = self.expr(handle, slots)?;
                return match found_slot(&mut pool, &handle)? {
                    Slot::Compound(entry) => Ok(entry.swap_remove(1)),
                    other => Err(Stop::Unsound(format!("a slot {other:?}"))),
                };
            }
            ExprKind::Top(stack) => {
                return match self.expr(stack, slots)? {
                    Slot::Compound(mut elements) => {
                        elements.pop().ok_or(Stop::Ended(Ending::Trapped))
                    }
                    other => Err(Stop::Unsound(format!("the top of {other:?}"))),
                };
            }
            /*
             * A view runs as a copy of the elements it shows when it is made.
             * The generators write no program that changes such an element
             * while the view is in use, where a copy and a view would differ.
             */
            ExprKind::View(items) => return self.expr(items, slots),
            ExprKind::Slice { items, start, end } => {
                return self.slice(items, start, end, slots);
            }
            ExprKind::Bytes(text) => {
                return match self.expr(text, slots)? {
                    Slot::Text(text) => Ok(Slot::Compound(
                        text.bytes()
                            .map(|byte| Slot::Integer(i128::from(byte)))
                            .collect(),
                    )),
                    other => Err(Stop::Unsound(format!("the bytes of {other:?}"))),
                };
            }
            ExprKind::Construct(fields) => {
                let count = match &expr.ty {
                    Type::Custom(custom) => custom.fields.len(),
                    other => return Err(Stop::Unsound(format!("a construction of {other}"))),
                };
                return Ok(Slot::Compound(self.fields(fields, count, slots)?));
            }
            ExprKind::Variant { variant, fields } => {
                let count = match &expr.ty {
                    Type::Choice(choice) => choice
                        .variants
                        .get(*variant)
                        .map(|declared| declared.fields.len()),
                    _ => None,
                }
                .ok_or_else(|| Stop::Unsound(format!("variant {variant} of {}", expr.ty)))?;
                return Ok(Slot::Variant {
                    variant: *variant,
                    fields: self.fields(fields, count, slots)?,
                });
            }
            ExprKind::Field { object, field } => {
                return match self.expr(object, slots)? {
                    Slot::Compound(mut values) if *field < values.len() => {
                        Ok(values.swap_remove(*field))
                    }
                    other => Err(Stop::Unsound(format!("field {field} of {other:?}"))),
                };
            }
            ExprKind::Length(operand) => {
                return match self.expr(operand, slots)? {
                    Slot::Compound(values) => Ok(Slot::Integer(values.len() as i128)),
                    Slot::Text(text) => Ok(Slot::Integer(text.len() as i128)),
                    other => Err(Stop::Unsound(format!("the length of {other:?}"))),
                };
            }
            ExprKind::Negate { operand, checked } => (
                self.integer(operand, slots)?.checked_neg(),
                *checked,
                "negation",
            ),
            ExprKind::Convert { operand, checked } => {
                (Some(self.integer(operand, slots)?), *checked, "conversion")
            }
            ExprKind::Arithmetic {
                operator,
                left,
                right,
                checked,
            } => {
                let left = self.integer(left, slots)?;
                let right = self.integer(right, slots)?;
                (
                    arithmetic(*operator, left, right, ty, *checked, expr)?,
                    *checked,
                    "arithmetic",
                )
            }
        };
        let ty = ty.ok_or_else(|| Stop::Unsound(format!("{what} without an integer type")))?;
        match result {
            Some(value) if ty.contains(value) => Ok(Slot::Integer(value)),
            _ if checked => Err(Stop::Ended(Ending::Trapped)),
            _ => {
                let result = result.map_or_else(
                    || "more than 128 bits".to_owned(),
                    |value| value.to_string(),
                );
                Err(Stop::Unsound(format!(
                    "unchecked {what} at {:?} gave {result}, outside `{}`",
                    expr.span,
                    ty.name()
                )))
            }
        }
    }
}

/**
 * The exact result of an arithmetic operation, or `None` when it is too large
 * for `i128`, which only a product of two large `unsigned64` values can be.
 * A divisor of zero traps, or fails the test when the check is gone.
 */
fn arithmetic(
    operator: ArithmeticOperator,
    left: i128,
    right: i128,
    ty: Option<IntegerType>,
    checked: bool,
    expr: &Expr,
) -> Eval<Option<i128>> {
    let divides = matches!(
        operator,
        ArithmeticOperator::Divide | ArithmeticOperator::Remainder
    );
    if divides && right == 0 {
        if checked {
            return Err(Stop::Ended(Ending::Trapped));
        }
        return Err(Stop::Unsound(format!(
            "unchecked division by zero at {:?}",
            expr.span
        )));
    }
    // C leaves even the remainder of the lowest value by -1 undefined, so neither may lose its check.
    let lowest = ty.filter(|ty| ty.is_signed()).map(IntegerType::minimum);
    if divides && !checked && right == -1 && Some(left) == lowest {
        return Err(Stop::Unsound(format!(
            "unchecked lowest value divided by -1 at {:?}",
            expr.span
        )));
    }
    Ok(match operator {
        ArithmeticOperator::Add => left.checked_add(right),
        ArithmeticOperator::Subtract => left.checked_sub(right),
        ArithmeticOperator::Multiply => left.checked_mul(right),
        ArithmeticOperator::Divide => left.checked_div(right),
        ArithmeticOperator::Remainder => left.checked_rem(right),
    })
}

/// The position an index selects in a value of `length` elements, or the trap a checked one causes.
fn check_index(index: i128, length: usize, checked: bool, span: Span) -> Eval<usize> {
    if let Ok(position) = usize::try_from(index) {
        if position < length {
            return Ok(position);
        }
    }
    if checked {
        return Err(Stop::Ended(Ending::Trapped));
    }
    Err(Stop::Unsound(format!(
        "unchecked index {index} at {span:?} is outside a value of length {length}"
    )))
}

/// A start or an end of a slice that lies from `low` to `length`, or else the program stops there.
fn slice_position(position: i128, low: usize, length: usize) -> Eval<usize> {
    match usize::try_from(position) {
        Ok(position) if (low..=length).contains(&position) => Ok(position),
        _ => Err(Stop::Ended(Ending::Trapped)),
    }
}

/// How many elements an array or a list holds now.
fn compound_length(slot: &Slot) -> Eval<usize> {
    match slot {
        Slot::Compound(values) => Ok(values.len()),
        other => Err(Stop::Unsound(format!("the elements of {other:?}"))),
    }
}

/// One step from a local to the array or list a `for` loop reads: an index with its check and place, or a field.
enum Stored<'expr> {
    Index(&'expr Expr, bool, Span),
    Field(usize),
}

/// The local an array expression names, and the steps that lead from it to the array, if it names one.
fn stored_path(expr: &Expr) -> Option<(LocalId, Vec<Stored<'_>>)> {
    match &expr.kind {
        ExprKind::Local(local) => Some((*local, Vec::new())),
        ExprKind::Index {
            array,
            index,
            checked,
            ..
        } => {
            let (local, mut steps) = stored_path(array)?;
            steps.push(Stored::Index(index, *checked, expr.span));
            Some((local, steps))
        }
        ExprKind::Field { object, field } => {
            let (local, mut steps) = stored_path(object)?;
            steps.push(Stored::Field(*field));
            Some((local, steps))
        }
        _ => None,
    }
}

/// The slot a handle names in `pool`, its generation and its value, or the trap a stale handle stops the program with.
fn found_slot<'pool>(pool: &'pool mut Slot, handle: &Slot) -> Eval<&'pool mut Slot> {
    let Slot::Compound(handle_parts) = handle else {
        return Err(Stop::Unsound(format!("a handle {handle:?}")));
    };
    let slot = slot_of(handle)?;
    let Slot::Compound(entries) = pool else {
        return Err(Stop::Unsound(format!("a pool {pool:?}")));
    };
    let entry = entries.get_mut(slot).ok_or(Stop::Ended(Ending::Trapped))?;
    let same = match (entry, handle_parts.get(1)) {
        (Slot::Compound(entry), Some(Slot::Integer(generation))) => {
            matches!(entry.first(), Some(Slot::Integer(current)) if current == generation)
        }
        _ => false,
    };
    if !same {
        return Err(Stop::Ended(Ending::Trapped));
    }
    entries.get_mut(slot).ok_or(Stop::Ended(Ending::Trapped))
}

/// The slot a handle names.
fn slot_of(handle: &Slot) -> Eval<usize> {
    match handle {
        Slot::Compound(parts) => match parts.first() {
            Some(Slot::Integer(slot)) => {
                usize::try_from(*slot).map_err(|_| Stop::Unsound(format!("a slot {slot}")))
            }
            _ => Err(Stop::Unsound(format!("a handle {handle:?}"))),
        },
        other => Err(Stop::Unsound(format!("a handle {other:?}"))),
    }
}

/// The element of `slot` at `positions`, one per level of nesting.
fn element_mut<'slot>(slot: &'slot mut Slot, positions: &[usize]) -> Eval<&'slot mut Slot> {
    let Some((&first, rest)) = positions.split_first() else {
        return Ok(slot);
    };
    match slot {
        Slot::Compound(values) => match values.get_mut(first) {
            Some(element) => element_mut(element, rest),
            None => Err(Stop::Unsound(format!("element {first} past the end"))),
        },
        other => Err(Stop::Unsound(format!("an element of {other:?}"))),
    }
}

fn from_value(value: &Value) -> Slot {
    match value {
        Value::Integer { value, .. } => Slot::Integer(*value),
        Value::Float { bits, .. } => Slot::Float(f64::from_bits(*bits)),
        Value::Boolean(value) => Slot::Boolean(*value),
        Value::Text(text) => Slot::Text(text.clone()),
    }
}

fn describe(slot: &Slot) -> String {
    match slot {
        Slot::Integer(value) => value.to_string(),
        Slot::Boolean(value) => value.to_string(),
        Slot::Text(text) => text.clone(),
        Slot::Float(_) | Slot::Compound(_) | Slot::Variant { .. } | Slot::Unset => "?".to_owned(),
    }
}

fn compare(operator: ComparisonOperator, left: &Slot, right: &Slot) -> bool {
    // NaN is unordered: only `!=` holds for it, as IEEE 754 and the generated C say.
    if let (Slot::Float(left), Slot::Float(right)) = (left, right) {
        return match operator {
            ComparisonOperator::Equal => left == right,
            ComparisonOperator::NotEqual => left != right,
            ComparisonOperator::Less => left < right,
            ComparisonOperator::LessEqual => left <= right,
            ComparisonOperator::Greater => left > right,
            ComparisonOperator::GreaterEqual => left >= right,
        };
    }
    let ordering = match (left, right) {
        (Slot::Integer(left), Slot::Integer(right)) => left.cmp(right),
        (Slot::Boolean(left), Slot::Boolean(right)) => left.cmp(right),
        (Slot::Text(left), Slot::Text(right)) => left.cmp(right),
        // Only choices whose variants hold no fields are compared, by their variant.
        (Slot::Variant { variant: left, .. }, Slot::Variant { variant: right, .. }) => {
            left.cmp(right)
        }
        // Only handles are compared among values that hold others: their slots and generations.
        (Slot::Compound(left), Slot::Compound(right)) => {
            let parts = |parts: &[Slot]| match parts {
                [Slot::Integer(slot), Slot::Integer(generation)] => Some((*slot, *generation)),
                _ => None,
            };
            match (parts(left), parts(right)) {
                (Some(left), Some(right)) => left.cmp(&right),
                _ => return false,
            }
        }
        _ => return false,
    };
    match operator {
        ComparisonOperator::Equal => ordering.is_eq(),
        ComparisonOperator::NotEqual => ordering.is_ne(),
        ComparisonOperator::Less => ordering.is_lt(),
        ComparisonOperator::LessEqual => ordering.is_le(),
        ComparisonOperator::Greater => ordering.is_gt(),
        ComparisonOperator::GreaterEqual => ordering.is_ge(),
    }
}
