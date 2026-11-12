/*!
 * Runs a checked program exactly, with every integer as an `i128`, and stops
 * at the first operation that fails without a check.
 */

use std::fmt::Write as _;

use lymra::ir::{
    Argument, ArithmeticOperator, Block, Call, ComparisonOperator, Expr, ExprKind, FunctionId,
    IntegerType, LocalId, LogicOperator, PlaceIndex, PrintPart, Program, Statement, Value,
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
    };
    Ok(Outcome {
        ending,
        output: interpreter.output,
    })
}

#[derive(Clone, Debug)]
enum Slot {
    Integer(i128),
    Boolean(bool),
    Text(String),
    Array(Vec<Slot>),
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
        let result = match self.block(&definition.body, &mut slots)? {
            Flow::Return(value) => value,
            Flow::Next | Flow::Break | Flow::Continue => None,
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
                Argument::Reference(local) => slots[*local].clone(),
            });
        }
        let (result, parameters) = self.call(call.function, arguments)?;
        for (argument, value) in call.arguments.iter().zip(parameters) {
            if let Argument::Reference(local) = argument {
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
                let positions = self.positions(&target.indexes, slots)?;
                let value = self.expr(value, slots)?;
                *element_mut(&mut slots[target.local], &positions)? = value;
            }
            Statement::Discard(value) => {
                self.expr(value, slots)?;
            }
            Statement::Call(call) => {
                self.call_expr(call, slots)?;
            }
            Statement::Print(parts) => {
                // Every value is computed before the line starts, as in the C the compiler makes.
                let mut line = String::new();
                for part in parts {
                    match part {
                        PrintPart::Text(text) => line.push_str(text),
                        PrintPart::Value { value, .. } => {
                            let value = self.expr(value, slots)?;
                            let _ = write!(line, "{}", describe(&value));
                        }
                    }
                }
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
                length,
                body,
            } => {
                let elements = self.elements(array, slots)?;
                for position in 0..*length {
                    self.step()?;
                    let position = usize::try_from(position)
                        .map_err(|_| Stop::Unsound("an array longer than memory".to_owned()))?;
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
            Statement::Return(value) => {
                let value = match value {
                    Some(value) => Some(self.expr(value, slots)?),
                    None => None,
                };
                return Ok(Flow::Return(value));
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

    /// Computes and checks the indexes of an assignment's target, in order.
    fn positions(&mut self, indexes: &[PlaceIndex], slots: &mut [Slot]) -> Eval<Vec<usize>> {
        let mut positions = Vec::with_capacity(indexes.len());
        for index in indexes {
            let value = self.integer(&index.value, slots)?;
            positions.push(check_index(value, index.length, index.checked, index.span)?);
        }
        Ok(positions)
    }

    /// Where a `for` loop over `array` reads its elements: storage when `array` names some, as the C does.
    fn elements(&mut self, array: &Expr, slots: &mut [Slot]) -> Eval<Elements> {
        match stored_path(array) {
            Some((local, indexes)) => {
                let mut positions = Vec::with_capacity(indexes.len());
                for (index, length, checked, span) in indexes {
                    let value = self.integer(index, slots)?;
                    positions.push(check_index(value, length, checked, span)?);
                }
                Ok(Elements::Stored { local, positions })
            }
            None => match self.expr(array, slots)? {
                Slot::Array(values) => Ok(Elements::Computed(values)),
                other => Err(Stop::Unsound(format!("a loop over {other:?}"))),
            },
        }
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
            ExprKind::Index {
                array,
                index,
                length,
                checked,
            } => {
                let array = self.expr(array, slots)?;
                let index = self.integer(index, slots)?;
                let position = check_index(index, *length, *checked, expr.span)?;
                return match array {
                    Slot::Array(mut values) if position < values.len() => {
                        Ok(values.swap_remove(position))
                    }
                    other => Err(Stop::Unsound(format!("element {position} of {other:?}"))),
                };
            }
            ExprKind::Fill(value) => {
                let value = self.expr(value, slots)?;
                let length = expr
                    .ty
                    .as_array()
                    .and_then(|array| usize::try_from(array.length).ok())
                    .ok_or_else(|| Stop::Unsound("a fill without an array type".to_owned()))?;
                return Ok(Slot::Array(vec![value; length]));
            }
            ExprKind::Length(operand) => {
                return match self.expr(operand, slots)? {
                    Slot::Array(values) => Ok(Slot::Integer(values.len() as i128)),
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

/// The position an index selects, or the trap a checked one causes.
fn check_index(index: i128, length: u64, checked: bool, span: Span) -> Eval<usize> {
    if (0..i128::from(length)).contains(&index) {
        return usize::try_from(index)
            .map_err(|_| Stop::Unsound("an array longer than memory".to_owned()));
    }
    if checked {
        return Err(Stop::Ended(Ending::Trapped));
    }
    Err(Stop::Unsound(format!(
        "unchecked index {index} at {span:?} is outside an array of length {length}"
    )))
}

type StoredIndex<'expr> = (&'expr Expr, u64, bool, Span);

/// The local an array expression names, and the indexes that lead from it to the array, if it names one.
fn stored_path(expr: &Expr) -> Option<(LocalId, Vec<StoredIndex<'_>>)> {
    match &expr.kind {
        ExprKind::Local(local) => Some((*local, Vec::new())),
        ExprKind::Index {
            array,
            index,
            length,
            checked,
        } => {
            let (local, mut indexes) = stored_path(array)?;
            indexes.push((index, *length, *checked, expr.span));
            Some((local, indexes))
        }
        _ => None,
    }
}

/// The element of `slot` at `positions`, one per level of nesting.
fn element_mut<'slot>(slot: &'slot mut Slot, positions: &[usize]) -> Eval<&'slot mut Slot> {
    let Some((&first, rest)) = positions.split_first() else {
        return Ok(slot);
    };
    match slot {
        Slot::Array(values) => match values.get_mut(first) {
            Some(element) => element_mut(element, rest),
            None => Err(Stop::Unsound(format!("element {first} past the end"))),
        },
        other => Err(Stop::Unsound(format!("an element of {other:?}"))),
    }
}

fn from_value(value: &Value) -> Slot {
    match value {
        Value::Integer { value, .. } => Slot::Integer(*value),
        Value::Boolean(value) => Slot::Boolean(*value),
        Value::Text(text) => Slot::Text(text.clone()),
    }
}

fn describe(slot: &Slot) -> String {
    match slot {
        Slot::Integer(value) => value.to_string(),
        Slot::Boolean(value) => value.to_string(),
        Slot::Text(text) => text.clone(),
        Slot::Array(_) | Slot::Unset => "?".to_owned(),
    }
}

fn compare(operator: ComparisonOperator, left: &Slot, right: &Slot) -> bool {
    let ordering = match (left, right) {
        (Slot::Integer(left), Slot::Integer(right)) => left.cmp(right),
        (Slot::Boolean(left), Slot::Boolean(right)) => left.cmp(right),
        (Slot::Text(left), Slot::Text(right)) => left.cmp(right),
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
