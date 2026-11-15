/*!
 * Interval analysis over one function, and the removal of the checks it proves unnecessary.
 *
 * The walk goes through the function once, in order, holding bounds for each
 * integer local. One invariant makes it sound: wherever the walk is, the
 * bounds it holds for a variable include every value the variable can have
 * there.
 *
 * - Straight-line code updates the bounds exactly.
 * - An `if` narrows the bounds by its conditions in each branch and joins the
 *   branches that continue afterward. An `assert` narrows them after it.
 * - A `for` loop runs a known number of times. Each variable it changes gets
 *   bounds that hold at every point of every iteration (see `loops.rs`), and
 *   the body is walked once more with those, to remove checks.
 * - A `while` loop is worked out to a fixed point at its head (see
 *   `while_loops.rs`), and its body is walked once more with that.
 * - Parameters start with the bounds of every call in the program, and a
 *   call gives the bounds of every value its function returns (see
 *   `facts.rs`).
 * - An index into a list is proven by a fact that ties two locals: an
 *   integer plus a constant is below the list's length. A `for` loop up to
 *   `length(items)` gives its variable one, and so do tests such as
 *   `index + 1 < length(items)`. The fact lasts until either local takes a
 *   new value; a `push` only makes the list longer, so it keeps the fact.
 *
 * Anything else is treated as any value of its type, such as a variable
 * changed in ways the analysis does not follow. Its mistakes can only keep a
 * check that could have gone.
 */

use std::collections::{BTreeMap, HashSet};

use super::facts::Facts;
use super::interval::Interval;
use super::loops::{self, Change, Changes, Followed};
use super::proof::{Bound, UnprovenIndex};
use crate::ir::{
    Argument, ArithmeticOperator, Block, Branch, Call, Case, ComparisonOperator, Expr, ExprKind,
    Extent, Function, FunctionId, IntegerType, Local, LocalId, LogicOperator, PlaceStep, PrintPart,
    Statement, Type, Value,
};
use crate::source::Span;

/**
 * How many times a loop's changes are worked out. A variable whose change
 * depends on another changed variable, as in `total = total + count`, is
 * bounded once that one is, in a later round.
 */
const REFINEMENT_ROUNDS: usize = 3;

/**
 * Rounds of a fixed point in which bounds grow as they are, before a bound
 * that still moves is widened to the end of its type. A loop that runs a few
 * times, or a recursion a few levels deep, settles within them exactly.
 */
pub(super) const WIDENING_DELAY: usize = 3;

/**
 * Body walks a function may spend working `while` loops out to a fixed
 * point. Nested loops multiply them; past the limit, a loop forgets what it
 * changes, as every loop once did, so compiling stays fast.
 */
const LOOP_BUDGET: usize = 1_000;

/// Removes the checks in one function that its bounds prove unnecessary, and returns the indexes it could not prove.
pub(super) fn optimize_function(
    function: &mut Function<'_>,
    id: FunctionId,
    constants: &[Value],
    facts: &Facts,
) -> Vec<UnprovenIndex> {
    let mut analyzer = Analyzer::new(&function.locals, constants, facts, None);
    let mut bounds = analyzer.entry(&function.parameters, id);
    analyzer.walk_block(
        &mut function.body,
        &mut bounds,
        &mut Changes::new(),
        Mode::Remove,
    );
    analyzer.unproven
}

/**
 * Records into `observed` what one function's calls pass and what it
 * returns, assuming `facts` about the others. The analysis runs as it does
 * to remove checks, on a copy, so it sees the same bounds.
 */
pub(super) fn observe_function(
    function: &Function<'_>,
    id: FunctionId,
    constants: &[Value],
    facts: &Facts,
    observed: &mut Facts,
) {
    let mut copy = function.clone();
    let mut analyzer = Analyzer::new(&function.locals, constants, facts, Some((id, observed)));
    let mut bounds = analyzer.entry(&function.parameters, id);
    analyzer.walk_block(
        &mut copy.body,
        &mut bounds,
        &mut Changes::new(),
        Mode::Remove,
    );
}

/**
 * What holds at one point of a function: bounds for each integer local, by
 * id, where `None` stands for any value of the local's type, and the
 * integers known to be below the length of a list.
 */
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Bounds {
    values: Vec<Option<Interval>>,
    /**
     * For an integer local and a list local, the largest known `offset`
     * with `integer + offset < length(list)`. A smaller offset holds too.
     */
    below_lengths: BTreeMap<(LocalId, LocalId), i128>,
}

impl Bounds {
    /// Nothing known about any of `locals` locals.
    fn unknown(locals: usize) -> Bounds {
        Bounds {
            values: vec![None; locals],
            below_lengths: BTreeMap::new(),
        }
    }

    pub fn get(&self, local: LocalId) -> Option<Interval> {
        self.values.get(local).copied().flatten()
    }

    pub fn set(&mut self, local: LocalId, bounds: Option<Interval>) {
        if let Some(slot) = self.values.get_mut(local) {
            *slot = bounds;
        }
    }

    /// The locals and their bounds, by id.
    pub fn iter(&self) -> impl Iterator<Item = (LocalId, Option<Interval>)> + '_ {
        self.values.iter().copied().enumerate()
    }

    /// Whether `index + offset` is known to be below the length of the list `list`.
    fn is_below_length(&self, index: LocalId, offset: i128, list: LocalId) -> bool {
        self.below_lengths
            .get(&(index, list))
            .is_some_and(|&known| known >= offset)
    }

    /// Records that `index + offset < length(list)` holds, keeping the stronger of what was known.
    fn set_below_length(&mut self, index: LocalId, offset: i128, list: LocalId) {
        let known = self.below_lengths.entry((index, list)).or_insert(offset);
        *known = (*known).max(offset);
    }

    /// Drops what ties `local` to another local, which no longer holds once `local` takes a new value.
    fn untie(&mut self, local: LocalId) {
        self.below_lengths
            .retain(|&(index, list), _| index != local && list != local);
    }

    /// Bounds that hold where both do: on a path that has passed both tests.
    pub fn meet(&self, other: &Bounds) -> Bounds {
        let values = self
            .values
            .iter()
            .zip(&other.values)
            .map(|(first, second)| match (first, second) {
                // An empty meet is a place no value reaches; either side holds there.
                (Some(first), Some(second)) => Some(first.intersect(*second).unwrap_or(*first)),
                (Some(only), None) | (None, Some(only)) => Some(*only),
                (None, None) => None,
            })
            .collect();
        let mut below_lengths = self.below_lengths.clone();
        for (&(index, list), &offset) in &other.below_lengths {
            let known = below_lengths.entry((index, list)).or_insert(offset);
            *known = (*known).max(offset);
        }
        Bounds {
            values,
            below_lengths,
        }
    }

    /// Whether every value these bounds allow, `other` allows too.
    pub fn within(&self, other: &Bounds) -> bool {
        let values_within =
            self.values
                .iter()
                .zip(&other.values)
                .all(|(inner, outer)| match (inner, outer) {
                    (_, None) => true,
                    (None, Some(_)) => false,
                    (Some(inner), Some(outer)) => {
                        outer.low <= inner.low && inner.high <= outer.high
                    }
                });
        values_within
            && other
                .below_lengths
                .iter()
                .all(|(&(index, list), &offset)| self.is_below_length(index, offset, list))
    }

    /// Bounds that hold on either of two paths.
    pub fn join(&self, other: &Bounds) -> Bounds {
        let values = self
            .values
            .iter()
            .zip(&other.values)
            .map(|(first, second)| match (first, second) {
                (Some(first), Some(second)) => Some(first.join(*second)),
                _ => None,
            })
            .collect();
        // A fact holds on both paths with the smaller of their offsets.
        let below_lengths = self
            .below_lengths
            .iter()
            .filter_map(|(&key, &offset)| {
                let other = other.below_lengths.get(&key)?;
                Some((key, offset.min(*other)))
            })
            .collect();
        Bounds {
            values,
            below_lengths,
        }
    }
}

/**
 * The variable of a counted loop: its local, its bounds in every iteration,
 * and a list it stays below the length of, with the offset it has to add to
 * reach it.
 */
#[derive(Clone, Copy)]
struct Counter {
    local: LocalId,
    bounds: Option<Interval>,
    below_length_of: Option<(LocalId, i128)>,
}

impl Counter {
    /// Gives the variable its value for an iteration.
    fn start_iteration(self, bounds: &mut Bounds) {
        bounds.set(self.local, self.bounds);
        bounds.untie(self.local);
        if let Some((list, offset)) = self.below_length_of {
            bounds.set_below_length(self.local, offset, list);
        }
    }
}

/// Whether control can reach the statement after this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flow {
    Continues,
    Leaves,
}

/// Where the iterations of a loop leave early: the bounds at its `break` statements, and at its `continue` ones.
#[derive(Default)]
pub(super) struct Exits {
    pub broken: Option<Bounds>,
    pub continued: Option<Bounds>,
}

/// Joins `bounds` into what is there, if anything is.
pub(super) fn join_into(slot: &mut Option<Bounds>, bounds: &Bounds) {
    *slot = Some(match slot.take() {
        Some(previous) => previous.join(bounds),
        None => bounds.clone(),
    });
}

/// What holds after the paths of an `if` or a `match` meet again.
#[derive(Default)]
struct Merge {
    /// Bounds that hold at the end of every path that continues, or `None` if none does.
    after: Option<Bounds>,
    /// What every path did to the followed variables, including paths that left.
    changes: Option<Changes>,
}

impl Merge {
    fn add(&mut self, bounds: Bounds, changes: Changes, flow: Flow) {
        if flow == Flow::Continues {
            self.after = Some(match self.after.take() {
                Some(previous) => previous.join(&bounds),
                None => bounds,
            });
        }
        // A path that leaves still changed things before it left, inside the current iteration.
        self.changes = Some(match self.changes.take() {
            Some(previous) => loops::join_changes(&previous, &changes),
            None => changes,
        });
    }
}

/// What a walk does besides tracking bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    /// Only learn what a loop body changes. Nothing is removed.
    Probe,
    /// Remove every check the bounds prove unnecessary, and record calls and returns when facts are being worked out.
    Remove,
}

pub(super) struct Analyzer<'function> {
    locals: &'function [Local<'function>],
    constants: &'function [Value],
    /// What every function of the program receives and returns.
    facts: &'function Facts,
    /// When facts are being worked out: the function analyzed, and where its calls and returns are recorded.
    observer: Option<(FunctionId, &'function mut Facts)>,
    /// Every index check that stays, with what is known about the index there.
    unproven: Vec<UnprovenIndex>,
    /// The loops around the current point, innermost last.
    exits: Vec<Exits>,
    /// Body walks left for working `while` loops out to a fixed point.
    pub budget: usize,
}

/// True if no value from the operand intervals can make the operation fail.
fn is_safe(operator: ArithmeticOperator, left: Interval, right: Interval, ty: IntegerType) -> bool {
    // `apply` gives no result when a divisor of zero is possible.
    let Some(result) = left.apply(operator, right) else {
        return false;
    };
    // The lowest value divided by -1 overflows, and in C even its remainder is undefined.
    let divides = matches!(
        operator,
        ArithmeticOperator::Divide | ArithmeticOperator::Remainder
    );
    let lowest_by_minus_one =
        divides && ty.is_signed() && left.contains(ty.minimum()) && right.contains(-1);
    result.fits(ty) && !lowest_by_minus_one
}

/// The comparison that holds when `operator` does not.
fn negated(operator: ComparisonOperator) -> ComparisonOperator {
    match operator {
        ComparisonOperator::Equal => ComparisonOperator::NotEqual,
        ComparisonOperator::NotEqual => ComparisonOperator::Equal,
        ComparisonOperator::Less => ComparisonOperator::GreaterEqual,
        ComparisonOperator::LessEqual => ComparisonOperator::Greater,
        ComparisonOperator::Greater => ComparisonOperator::LessEqual,
        ComparisonOperator::GreaterEqual => ComparisonOperator::Less,
    }
}

/// The same comparison with its sides swapped: `a < b` is `b > a`.
fn mirrored(operator: ComparisonOperator) -> ComparisonOperator {
    match operator {
        ComparisonOperator::Equal | ComparisonOperator::NotEqual => operator,
        ComparisonOperator::Less => ComparisonOperator::Greater,
        ComparisonOperator::LessEqual => ComparisonOperator::GreaterEqual,
        ComparisonOperator::Greater => ComparisonOperator::Less,
        ComparisonOperator::GreaterEqual => ComparisonOperator::LessEqual,
    }
}

impl<'function> Analyzer<'function> {
    fn new(
        locals: &'function [Local<'function>],
        constants: &'function [Value],
        facts: &'function Facts,
        observer: Option<(FunctionId, &'function mut Facts)>,
    ) -> Self {
        Analyzer {
            locals,
            constants,
            facts,
            observer,
            unproven: Vec::new(),
            exits: Vec::new(),
            budget: LOOP_BUDGET,
        }
    }

    /// Bounds where the function starts: its parameters have the bounds of every call to it.
    fn entry(&self, parameters: &[LocalId], function: FunctionId) -> Bounds {
        let mut bounds = Bounds::unknown(self.locals.len());
        if self.facts.is_called(function) {
            for (position, &local) in parameters.iter().enumerate() {
                bounds.set(local, self.facts.parameter(function, position));
            }
        }
        bounds
    }

    /// Records a call with the bounds of its arguments, when facts are being worked out.
    fn observe_call(&mut self, call: &Call, bounds: &Bounds) {
        if self.observer.is_none() {
            return;
        }
        let arguments: Vec<Option<Interval>> = call
            .arguments
            .iter()
            .map(|argument| match argument {
                Argument::Value(value) => self.range(value, bounds),
                Argument::Reference { local, .. } => self.local_range(bounds, *local),
            })
            .collect();
        if let Some((_, observed)) = self.observer.as_mut() {
            observed.observe_call(call.function, &arguments);
        }
    }

    /// Records every call inside `expr`, with the bounds of its arguments.
    fn observe_calls(&mut self, expr: &Expr, bounds: &Bounds) {
        let mut calls = Vec::new();
        loops::collect_calls(expr, &mut calls);
        for call in calls {
            self.observe_call(call, bounds);
        }
    }

    /// Records a `return`, when facts are being worked out.
    fn observe_return(&mut self, value: &Expr, bounds: &Bounds) {
        let range = self.range(value, bounds);
        if let Some((function, observed)) = self.observer.as_mut() {
            observed.observe_return(*function, range);
        }
    }

    /// Walks a loop's body once, and returns how it ends and where it leaves early.
    pub fn walk_loop_body(
        &mut self,
        body: &mut Block,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) -> (Flow, Exits) {
        self.exits.push(Exits::default());
        let flow = self.walk_block(body, bounds, changes, mode);
        let exits = self.exits.pop().unwrap_or_default();
        (flow, exits)
    }

    pub fn integer_type(&self, local: LocalId) -> Option<IntegerType> {
        self.locals
            .get(local)
            .and_then(|local| local.ty.as_integer())
    }

    /// The bounds of an integer local: known ones, or else its whole type.
    fn local_range(&self, bounds: &Bounds, local: LocalId) -> Option<Interval> {
        let ty = self.integer_type(local)?;
        Some(bounds.get(local).unwrap_or(Interval::of_type(ty)))
    }

    /// Bounds on the value of an integer expression, or `None` for any other expression.
    fn range(&self, expr: &Expr, bounds: &Bounds) -> Option<Interval> {
        let ty = expr.ty.as_integer()?;
        let whole_type = Interval::of_type(ty);
        let ideal = match &expr.kind {
            ExprKind::Literal(Value::Integer { value, .. }) => Interval::exact(*value),
            ExprKind::Local(local) => return self.local_range(bounds, *local),
            ExprKind::Constant(id) => match self.constants.get(*id) {
                Some(Value::Integer { value, .. }) => Interval::exact(*value),
                _ => whole_type,
            },
            ExprKind::Negate { operand, .. } => self
                .range(operand, bounds)
                .map_or(whole_type, Interval::negate),
            ExprKind::Arithmetic {
                operator,
                left,
                right,
                ..
            } => match (self.range(left, bounds), self.range(right, bounds)) {
                (Some(value), Some(_)) if squared_local(expr).is_some() => value.square(),
                (Some(left), Some(right)) => left.apply(*operator, right).unwrap_or(whole_type),
                _ => whole_type,
            },
            ExprKind::Convert { operand, .. } => self.range(operand, bounds).unwrap_or(whole_type),
            // A wrapping result is the exact one when that fits the type, and otherwise could be any value of it.
            ExprKind::Wrapping {
                operator,
                left,
                right,
            } => match (self.range(left, bounds), self.range(right, bounds)) {
                (Some(left), Some(right)) => left
                    .apply(operator.arithmetic(), right)
                    .filter(|exact| exact.fits(ty))
                    .unwrap_or(whole_type),
                _ => whole_type,
            },
            ExprKind::Length(operand) => match &operand.ty {
                Type::Array(array) => Interval::exact(i128::from(array.length)),
                Type::Integer(_)
                | Type::Float(_)
                | Type::Boolean
                | Type::String
                | Type::Custom(_)
                | Type::Choice(_)
                | Type::List(_)
                | Type::View(_) => Interval {
                    low: 0,
                    high: whole_type.high,
                },
            },
            // A call returns what its function's `return` statements give.
            ExprKind::Call(call) => self.facts.result(call.function).unwrap_or(whole_type),
            ExprKind::Literal(_)
            | ExprKind::Not(_)
            | ExprKind::Compare { .. }
            | ExprKind::Logic { .. }
            | ExprKind::Index { .. }
            | ExprKind::Fill(_)
            | ExprKind::View(_)
            | ExprKind::Slice { .. }
            | ExprKind::Bytes(_)
            | ExprKind::EmptyList
            | ExprKind::Construct(_)
            | ExprKind::Variant { .. }
            | ExprKind::Field { .. } => whole_type,
        };
        // A checked operation that does not stop the program gives a value of its type.
        Some(ideal.clamp(ty))
    }

    /// Bounds on the sum of additive terms, or `None` if a term is not an integer.
    fn sum_range(&self, terms: &[loops::Term<'_>], bounds: &Bounds) -> Option<Interval> {
        terms.iter().try_fold(Interval::exact(0), |sum, term| {
            let range = self.range(term.expr, bounds)?;
            Some(sum.add(if term.positive { range } else { range.negate() }))
        })
    }

    /**
     * Clears an index check when every value the index can have lies inside
     * its array or list, and otherwise records what is known about the index.
     * `list` is the list indexed, when that is a local.
     */
    fn prove_index(
        &mut self,
        index: &Expr,
        bound: Bound,
        list: Option<LocalId>,
        checked: &mut bool,
        bounds: &Bounds,
    ) {
        if !*checked {
            return;
        }
        let range = self.range(index, bounds);
        let proven = match bound {
            Bound::Fixed(length) => {
                range.is_some_and(|range| range.low >= 0 && range.high < i128::from(length))
            }
            // Only the running program knows a list's length, so an index needs a fact that ties it to the list.
            Bound::List(_) => {
                let below = match (self.offset_local(index, bounds), list) {
                    (Some((local, offset)), Some(list)) => {
                        bounds.is_below_length(local, offset, list)
                    }
                    _ => false,
                };
                below && range.is_some_and(|range| range.low >= 0)
            }
        };
        if proven {
            *checked = false;
        } else {
            self.unproven.push(UnprovenIndex::new(index, range, bound));
        }
    }

    /// Clears the checks of an expression's operations that cannot fail with values in these bounds.
    fn remove_checks(&mut self, expr: &mut Expr, bounds: &Bounds) {
        match &mut expr.kind {
            ExprKind::Literal(_)
            | ExprKind::Local(_)
            | ExprKind::Constant(_)
            | ExprKind::EmptyList => {}
            ExprKind::Negate { operand, .. }
            | ExprKind::Not(operand)
            | ExprKind::Convert { operand, .. }
            | ExprKind::Fill(operand)
            | ExprKind::Length(operand)
            | ExprKind::View(operand)
            | ExprKind::Bytes(operand)
            | ExprKind::Field {
                object: operand, ..
            } => {
                self.remove_checks(operand, bounds);
            }
            ExprKind::Slice { items, start, end } => {
                self.remove_checks(items, bounds);
                self.remove_checks(start, bounds);
                self.remove_checks(end, bounds);
            }
            ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => {
                for field in fields {
                    self.remove_checks(&mut field.value, bounds);
                }
            }
            ExprKind::Logic {
                operator,
                left,
                right,
            } => {
                self.remove_checks(left, bounds);
                // The right side runs only when the left one leaves the result open: true for `and`, false for `or`.
                let mut open = bounds.clone();
                self.refine(&mut open, left, *operator == LogicOperator::And);
                self.remove_checks(right, &open);
            }
            ExprKind::Arithmetic { left, right, .. }
            | ExprKind::Compare { left, right, .. }
            | ExprKind::Wrapping { left, right, .. }
            | ExprKind::Index {
                array: left,
                index: right,
                ..
            } => {
                self.remove_checks(left, bounds);
                self.remove_checks(right, bounds);
            }
            ExprKind::Call(call) => self.remove_argument_checks(call, bounds),
        }
        if let ExprKind::Index {
            array,
            index,
            length,
            checked,
        } = &mut expr.kind
        {
            let bound = Bound::of(*length, array.span);
            let list = match array.kind {
                ExprKind::Local(local) => Some(local),
                _ => None,
            };
            self.prove_index(index, bound, list, checked, bounds);
            return;
        }
        let Some(ty) = expr.ty.as_integer() else {
            return;
        };
        let safe = match &expr.kind {
            ExprKind::Arithmetic {
                operator,
                left,
                right,
                ..
            } => match (self.range(left, bounds), self.range(right, bounds)) {
                (Some(left), Some(right)) => is_safe(*operator, left, right, ty),
                _ => false,
            },
            ExprKind::Negate { operand, .. } => self
                .range(operand, bounds)
                .is_some_and(|range| range.negate().fits(ty)),
            ExprKind::Convert { operand, .. } => self
                .range(operand, bounds)
                .is_some_and(|range| range.fits(ty)),
            _ => false,
        };
        if !safe {
            return;
        }
        if let ExprKind::Arithmetic { checked, .. }
        | ExprKind::Negate { checked, .. }
        | ExprKind::Convert { checked, .. } = &mut expr.kind
        {
            *checked = false;
        }
    }

    fn remove_argument_checks(&mut self, call: &mut Call, bounds: &Bounds) {
        for argument in &mut call.arguments {
            if let Argument::Value(value) = argument {
                self.remove_checks(value, bounds);
            }
        }
    }

    /**
     * Handles an expression the program evaluates: removes its checks when
     * that is the job, then forgets what is known about every local it
     * passes with `variable`, since the called function may change it.
     */
    pub fn evaluate(
        &mut self,
        expr: &mut Expr,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) {
        if mode == Mode::Remove {
            self.observe_calls(expr, bounds);
            self.remove_checks(expr, bounds);
        }
        let mut passed = HashSet::new();
        loops::collect_references(expr, &mut passed);
        forget(&passed, bounds, changes);
    }

    fn walk_block(
        &mut self,
        block: &mut Block,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) -> Flow {
        for statement in &mut block.statements {
            if self.walk_statement(statement, bounds, changes, mode) == Flow::Leaves {
                return Flow::Leaves;
            }
        }
        Flow::Continues
    }

    fn walk_statement(
        &mut self,
        statement: &mut Statement,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) -> Flow {
        match statement {
            Statement::Declare { local, value } => {
                let range = self.range(value, bounds);
                self.evaluate(value, bounds, changes, mode);
                bounds.set(*local, range);
                bounds.untie(*local);
            }
            Statement::Assign { target, value } if target.steps.is_empty() => {
                let range = self.range(value, bounds);
                let step = loops::additive_terms(value, target.local)
                    .and_then(|terms| self.sum_range(&terms, bounds));
                self.evaluate(value, bounds, changes, mode);
                assign(target.local, range, step, bounds, changes);
            }
            // A part of an array or of a declared type changes, which no bounds the analysis holds describe.
            Statement::Assign { target, value } => {
                for (position, step) in target.steps.iter_mut().enumerate() {
                    if let PlaceStep::Index(index) = step {
                        if mode == Mode::Remove {
                            // The part of the place before this index is what it indexes.
                            let indexed = Span::new(index.span.start, index.value.span.start);
                            // Only the first step indexes the local itself.
                            let list = (position == 0).then_some(target.local);
                            self.prove_index(
                                &index.value,
                                Bound::of(index.length, indexed),
                                list,
                                &mut index.checked,
                                bounds,
                            );
                        }
                        self.evaluate(&mut index.value, bounds, changes, mode);
                    }
                }
                self.evaluate(value, bounds, changes, mode);
            }
            Statement::Discard(value) | Statement::Push { value, .. } => {
                self.evaluate(value, bounds, changes, mode);
            }
            Statement::Call(call) => {
                if mode == Mode::Remove {
                    for argument in &call.arguments {
                        if let Argument::Value(value) = argument {
                            self.observe_calls(value, bounds);
                        }
                    }
                    self.observe_call(call, bounds);
                    self.remove_argument_checks(call, bounds);
                }
                let mut passed = HashSet::new();
                for argument in &call.arguments {
                    match argument {
                        Argument::Reference { local, .. } => {
                            passed.insert(*local);
                        }
                        Argument::Value(value) => loops::collect_references(value, &mut passed),
                    }
                }
                forget(&passed, bounds, changes);
            }
            Statement::Print(parts) => {
                for part in parts {
                    if let PrintPart::Value { value, .. } = part {
                        self.evaluate(value, bounds, changes, mode);
                    }
                }
            }
            Statement::If {
                branches,
                otherwise,
            } => {
                return self.walk_if(branches, otherwise.as_mut(), bounds, changes, mode);
            }
            Statement::While { condition, body } => {
                self.walk_while(condition, body, bounds, changes, mode)
            }
            Statement::ForRange {
                variable,
                start,
                end,
                body,
            } => self.walk_for(*variable, start, end, body, bounds, changes, mode),
            Statement::ForEach {
                variable,
                array,
                length,
                body,
            } => {
                self.evaluate(array, bounds, changes, mode);
                // The loop runs once for each element, and its variable can be any value of their type.
                let trips = match length {
                    Extent::Fixed(length) => i128::from(*length),
                    // A list holds fewer elements than an `integer64` can count.
                    Extent::Dynamic => i128::from(i64::MAX),
                };
                let changed = loops::changed_outside(body);
                let element = variable.map(|local| Counter {
                    local,
                    bounds: None,
                    below_length_of: None,
                });
                self.walk_counted(body, element, trips, &changed, bounds, changes, mode);
            }
            Statement::Match { value, cases } => {
                self.evaluate(value, bounds, changes, mode);
                return self.walk_match(cases, bounds, changes, mode);
            }
            Statement::Assert { condition } => {
                self.evaluate(condition, bounds, changes, mode);
                // The program stops when an assertion fails, so after one its condition holds.
                self.refine(bounds, condition, true);
            }
            Statement::Return(value) => {
                if let Some(value) = value {
                    if mode == Mode::Remove {
                        self.observe_return(value, bounds);
                    }
                    self.evaluate(value, bounds, changes, mode);
                }
                return Flow::Leaves;
            }
            Statement::Break => {
                if let Some(exits) = self.exits.last_mut() {
                    join_into(&mut exits.broken, bounds);
                }
                return Flow::Leaves;
            }
            Statement::Continue => {
                if let Some(exits) = self.exits.last_mut() {
                    join_into(&mut exits.continued, bounds);
                }
                return Flow::Leaves;
            }
        }
        Flow::Continues
    }

    fn walk_if(
        &mut self,
        branches: &mut [Branch],
        otherwise: Option<&mut Block>,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) -> Flow {
        // `remaining` holds where every condition so far was false.
        let mut remaining = bounds.clone();
        let mut merge = Merge::default();
        for branch in branches.iter_mut() {
            self.evaluate(&mut branch.condition, &mut remaining, changes, mode);
            let mut inside = remaining.clone();
            self.refine(&mut inside, &branch.condition, true);
            let mut branch_changes = changes.clone();
            let flow = self.walk_block(&mut branch.body, &mut inside, &mut branch_changes, mode);
            merge.add(inside, branch_changes, flow);
            self.refine(&mut remaining, &branch.condition, false);
        }
        match otherwise {
            Some(block) => {
                let mut inside = remaining.clone();
                let mut branch_changes = changes.clone();
                let flow = self.walk_block(block, &mut inside, &mut branch_changes, mode);
                merge.add(inside, branch_changes, flow);
            }
            None => merge.add(remaining.clone(), changes.clone(), Flow::Continues),
        }
        self.finish_merge(merge, bounds, changes)
    }

    /**
     * A `match` runs exactly one of its cases, each from what holds before
     * it. A case's bindings can hold any value of their fields' types.
     */
    fn walk_match(
        &mut self,
        cases: &mut [Case],
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) -> Flow {
        let mut merge = Merge::default();
        for case in cases.iter_mut() {
            let mut inside = bounds.clone();
            for binding in &case.bindings {
                inside.set(binding.local, None);
                inside.untie(binding.local);
            }
            let mut case_changes = changes.clone();
            let flow = self.walk_block(&mut case.body, &mut inside, &mut case_changes, mode);
            merge.add(inside, case_changes, flow);
        }
        self.finish_merge(merge, bounds, changes)
    }

    /// Takes what holds where the paths meet. Control leaves when no path continues.
    fn finish_merge(&self, merge: Merge, bounds: &mut Bounds, changes: &mut Changes) -> Flow {
        if let Some(joined) = merge.changes {
            *changes = joined;
        }
        match merge.after {
            Some(after) => {
                *bounds = after;
                Flow::Continues
            }
            None => Flow::Leaves,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_for(
        &mut self,
        variable: Option<LocalId>,
        start: &mut Expr,
        end: &mut Expr,
        body: &mut Block,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) {
        let start_range = self.range(start, bounds);
        let end_range = self.range(end, bounds);
        self.evaluate(start, bounds, changes, mode);
        self.evaluate(end, bounds, changes, mode);
        let changed = loops::changed_outside(body);
        // `range(start, length(items) - 1)` stops below what `length(items) - 1` is when the loop starts, and a list that gets no shorter keeps it.
        let below_length_of = self
            .length_less(end, bounds)
            .filter(|&(list, _)| !loops::shortened_outside(body).contains(&list));
        let (Some(start_range), Some(end_range)) = (start_range, end_range) else {
            // Bounds the analysis cannot follow: the body still runs, any number of times, so its calls count.
            forget(&changed, bounds, changes);
            let mut inside = bounds.clone();
            if let Some(local) = variable {
                Counter {
                    local,
                    bounds: None,
                    below_length_of,
                }
                .start_iteration(&mut inside);
            }
            self.walk_loop_body(body, &mut inside, &mut Changes::new(), mode);
            return;
        };
        let trips = loops::trip_count(start_range, end_range);
        let value = Interval {
            low: start_range.low,
            high: end_range.high - 1,
        };
        let counter = variable.map(|local| Counter {
            local,
            bounds: Some(value),
            below_length_of,
        });
        self.walk_counted(body, counter, trips, &changed, bounds, changes, mode);
    }

    /**
     * A loop that runs at most `trips` times, with a variable that has the
     * given bounds in every iteration. Each local the body changes gets
     * bounds that hold however many of the iterations have run.
     */
    #[allow(clippy::too_many_arguments)]
    fn walk_counted(
        &mut self,
        body: &mut Block,
        counter: Option<Counter>,
        trips: i128,
        changed: &HashSet<LocalId>,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) {
        if trips == 0 {
            // The body never runs, so it changes nothing.
            return;
        }
        let rounds = if mode == Mode::Remove {
            REFINEMENT_ROUNDS
        } else {
            1
        };
        let LoopBounds {
            loop_wide,
            entry,
            extents,
        } = self.loop_bounds(body, counter, trips, changed, bounds, rounds);

        // Tell an enclosing loop being probed what this whole loop does to the variables it follows.
        for local in changed {
            if let Some(followed) = changes.get_mut(local) {
                followed.change = match extents
                    .iter()
                    .find(|(id, _)| id == local)
                    .and_then(|(_, extent)| *extent)
                {
                    Some(extent) => followed.change.add(extent.repeat(trips)),
                    None => Change::Unknown,
                };
            }
        }

        if mode == Mode::Remove {
            let mut inside = entry;
            if let Some(counter) = counter {
                counter.start_iteration(&mut inside);
            }
            self.walk_loop_body(body, &mut inside, &mut Changes::new(), Mode::Remove);
        }
        // After the loop, every variable it changed is still within its loop-wide bounds.
        for &local in changed {
            let after = match changes.get(&local) {
                Some(followed) => followed.loop_wide,
                None => loop_wide.get(local),
            };
            bounds.set(local, after);
            bounds.untie(local);
        }
    }

    /**
     * Works out bounds for each variable the loop body changes: bounds that
     * hold at every point of every iteration, and tighter ones that hold
     * where each iteration starts. Each round of probing uses the bounds of
     * the round before, which lets a change that depends on another changed
     * variable be bounded once that one is.
     */
    #[allow(clippy::too_many_arguments)]
    fn loop_bounds(
        &mut self,
        body: &mut Block,
        counter: Option<Counter>,
        trips: i128,
        changed: &HashSet<LocalId>,
        bounds: &Bounds,
        rounds: usize,
    ) -> LoopBounds {
        let mut loop_wide = bounds.clone();
        forget(changed, &mut loop_wide, &mut Changes::new());
        let mut entry = loop_wide.clone();
        let mut extents = Vec::new();
        for _ in 0..rounds {
            let mut probe = entry.clone();
            if let Some(counter) = counter {
                counter.start_iteration(&mut probe);
            }
            let mut changes: Changes = changed
                .iter()
                .map(|&local| {
                    let followed = Followed {
                        change: Change::NONE,
                        loop_wide: loop_wide.get(local),
                    };
                    (local, followed)
                })
                .collect();
            self.walk_loop_body(body, &mut probe, &mut changes, Mode::Probe);

            extents = changed
                .iter()
                .map(|&local| {
                    (
                        local,
                        changes
                            .get(&local)
                            .and_then(|followed| followed.change.extent()),
                    )
                })
                .collect();
            let mut next_wide = bounds.clone();
            let mut next_entry = bounds.clone();
            for &(local, extent) in &extents {
                let after = |repeats: i128| {
                    let ty = self.integer_type(local)?;
                    let before = self.local_range(bounds, local)?;
                    Some(before.add(extent?.repeat(repeats)).clamp(ty))
                };
                next_wide.set(local, after(trips));
                // An iteration starts after at most `trips - 1` others have finished.
                next_entry.set(local, after(trips - 1));
            }
            if next_wide == loop_wide && next_entry == entry {
                break;
            }
            loop_wide = next_wide;
            entry = next_entry;
        }
        LoopBounds {
            loop_wide,
            entry,
            extents,
        }
    }

    /// Narrows the bounds to where `condition` has the value `outcome`.
    pub fn refine(&self, bounds: &mut Bounds, condition: &Expr, outcome: bool) {
        match &condition.kind {
            ExprKind::Not(inner) => self.refine(bounds, inner, !outcome),
            ExprKind::Logic {
                operator: LogicOperator::And,
                left,
                right,
            } if outcome => {
                self.refine(bounds, left, true);
                self.refine(bounds, right, true);
            }
            ExprKind::Logic {
                operator: LogicOperator::Or,
                left,
                right,
            } if !outcome => {
                self.refine(bounds, left, false);
                self.refine(bounds, right, false);
            }
            ExprKind::Compare {
                operator,
                left,
                right,
            } => {
                let operator = if outcome {
                    *operator
                } else {
                    negated(*operator)
                };
                if let ExprKind::Local(local) = left.kind {
                    if let Some(other) = self.range(right, bounds) {
                        self.narrow(bounds, local, operator, other);
                    }
                }
                if let ExprKind::Local(local) = right.kind {
                    if let Some(other) = self.range(left, bounds) {
                        self.narrow(bounds, local, mirrored(operator), other);
                    }
                }
                self.tie_below_length(bounds, left, operator, right);
                self.tie_below_length(bounds, right, mirrored(operator), left);
                if let Some(local) = squared_local(left) {
                    if let Some(other) = self.range(right, bounds) {
                        self.narrow_square(bounds, local, operator, other);
                    }
                }
                if let Some(local) = squared_local(right) {
                    if let Some(other) = self.range(left, bounds) {
                        self.narrow_square(bounds, local, mirrored(operator), other);
                    }
                }
            }
            _ => {}
        }
    }

    /**
     * Records that an integer is below a list's length where `index operator
     * length` holds, `index` is a local plus a constant and `length` is a
     * list's length less a constant, as in `position + 1 < length(items)`.
     * Each side is exact where the comparison runs: a checked operation that
     * overflowed would have stopped the program.
     */
    fn tie_below_length(
        &self,
        bounds: &mut Bounds,
        index: &Expr,
        operator: ComparisonOperator,
        length: &Expr,
    ) {
        let (Some((local, added)), Some((list, subtracted))) = (
            self.offset_local(index, bounds),
            self.length_less(length, bounds),
        ) else {
            return;
        };
        // `local + added < length - subtracted` is `local + added + subtracted < length`; `<=` allows one less.
        let offset = match operator {
            ComparisonOperator::Less => added + subtracted,
            ComparisonOperator::LessEqual => added + subtracted - 1,
            _ => return,
        };
        bounds.set_below_length(local, offset, list);
    }

    /// A local plus a constant, as in `position + 1` or `position - 1`: the local and the constant.
    fn offset_local(&self, expr: &Expr, bounds: &Bounds) -> Option<(LocalId, i128)> {
        match &expr.kind {
            ExprKind::Local(local) => Some((*local, 0)),
            // A conversion that does not stop the program keeps the value.
            ExprKind::Convert { operand, .. } => self.offset_local(operand, bounds),
            ExprKind::Arithmetic {
                operator: ArithmeticOperator::Add,
                left,
                right,
                ..
            } => match (&left.kind, &right.kind) {
                (ExprKind::Local(local), _) => Some((*local, self.exact(right, bounds)?)),
                (_, ExprKind::Local(local)) => Some((*local, self.exact(left, bounds)?)),
                _ => None,
            },
            ExprKind::Arithmetic {
                operator: ArithmeticOperator::Subtract,
                left,
                right,
                ..
            } => match left.kind {
                ExprKind::Local(local) => Some((local, -self.exact(right, bounds)?)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The length of a list or a view less a constant, as in `length(items) - 1`: the list or view and the constant.
    fn length_less(&self, expr: &Expr, bounds: &Bounds) -> Option<(LocalId, i128)> {
        match &expr.kind {
            ExprKind::Length(operand) => match operand.kind {
                ExprKind::Local(list) if operand.ty.dynamic_elements().is_some() => Some((list, 0)),
                _ => None,
            },
            ExprKind::Arithmetic {
                operator: ArithmeticOperator::Subtract,
                left,
                right,
                ..
            } => {
                let (list, less) = self.length_less(left, bounds)?;
                Some((list, less + self.exact(right, bounds)?))
            }
            ExprKind::Arithmetic {
                operator: ArithmeticOperator::Add,
                left,
                right,
                ..
            } => {
                let (list, less) = self.length_less(left, bounds)?;
                Some((list, less - self.exact(right, bounds)?))
            }
            _ => None,
        }
    }

    /// The one value an integer expression can have, if the bounds pin it down.
    fn exact(&self, expr: &Expr, bounds: &Bounds) -> Option<i128> {
        self.range(expr, bounds)
            .filter(|range| range.low == range.high)
            .map(|range| range.low)
    }

    /**
     * Narrows a local to where `local * local operator other` holds, as in
     * `while divisor * divisor <= candidate`. The multiplication is checked,
     * so wherever the comparison runs, the square is exact.
     */
    fn narrow_square(
        &self,
        bounds: &mut Bounds,
        local: LocalId,
        operator: ComparisonOperator,
        other: Interval,
    ) {
        let Some(current) = self.local_range(bounds, local) else {
            return;
        };
        let squares = match operator {
            ComparisonOperator::Less => Interval {
                low: i128::MIN,
                high: other.high.saturating_sub(1),
            },
            ComparisonOperator::LessEqual => Interval {
                low: i128::MIN,
                high: other.high,
            },
            ComparisonOperator::Greater => Interval {
                low: other.low.saturating_add(1),
                high: i128::MAX,
            },
            ComparisonOperator::GreaterEqual => Interval {
                low: other.low,
                high: i128::MAX,
            },
            ComparisonOperator::Equal => other,
            ComparisonOperator::NotEqual => return,
        };
        // No value of the local can make it hold there, so the bounds stay as they are, as in `narrow`.
        if let Some(roots) = current.square_roots(squares) {
            bounds.set(local, Some(roots));
        }
    }

    /// Narrows one local to where `local operator other` holds. An impossible result leaves the bounds as they are.
    fn narrow(
        &self,
        bounds: &mut Bounds,
        local: LocalId,
        operator: ComparisonOperator,
        other: Interval,
    ) {
        let Some(current) = self.local_range(bounds, local) else {
            return;
        };
        let (mut low, mut high) = (current.low, current.high);
        match operator {
            ComparisonOperator::Less => high = high.min(other.high.saturating_sub(1)),
            ComparisonOperator::LessEqual => high = high.min(other.high),
            ComparisonOperator::Greater => low = low.max(other.low.saturating_add(1)),
            ComparisonOperator::GreaterEqual => low = low.max(other.low),
            ComparisonOperator::Equal => {
                low = low.max(other.low);
                high = high.min(other.high);
            }
            ComparisonOperator::NotEqual if other.low == other.high => {
                // A single excluded value can only shrink the interval at one of its ends.
                if other.low == low {
                    low += 1;
                } else if other.low == high {
                    high -= 1;
                }
            }
            ComparisonOperator::NotEqual => {}
        }
        if low <= high {
            bounds.set(local, Some(Interval { low, high }));
        }
    }
}

/// The local `expr` multiplies by itself, as in `divisor * divisor`, if it does.
fn squared_local(expr: &Expr) -> Option<LocalId> {
    let ExprKind::Arithmetic {
        operator: ArithmeticOperator::Multiply,
        left,
        right,
        ..
    } = &expr.kind
    else {
        return None;
    };
    match (&left.kind, &right.kind) {
        (ExprKind::Local(first), ExprKind::Local(second)) if first == second => Some(*first),
        _ => None,
    }
}

/// What `loop_bounds` works out for the variables a `for` loop changes.
struct LoopBounds {
    /// Bounds that hold at every point of every iteration, and after the loop.
    loop_wide: Bounds,
    /// Bounds that hold where each iteration starts.
    entry: Bounds,
    /// How far one iteration can move each variable, where only additive changes move it.
    extents: Vec<(LocalId, Option<Interval>)>,
}

/// Records an assignment. A variable a loop probe follows keeps its loop-wide bounds; any other gets the value's.
fn assign(
    target: LocalId,
    range: Option<Interval>,
    step: Option<Interval>,
    bounds: &mut Bounds,
    changes: &mut Changes,
) {
    match changes.get_mut(&target) {
        Some(followed) => {
            followed.change = match step {
                Some(step) => followed.change.add(step),
                None => Change::Unknown,
            };
            bounds.set(target, followed.loop_wide);
        }
        None => bounds.set(target, range),
    }
    bounds.untie(target);
}

/// Forgets what is known about locals that something may have changed in an unknown way.
pub(super) fn forget(locals: &HashSet<LocalId>, bounds: &mut Bounds, changes: &mut Changes) {
    for &local in locals {
        bounds.set(local, None);
        bounds.untie(local);
    }
    unfollow(locals, changes);
}

/// Tells a loop being probed that these locals change in ways it cannot add up.
pub(super) fn unfollow(locals: &HashSet<LocalId>, changes: &mut Changes) {
    for local in locals {
        if let Some(followed) = changes.get_mut(local) {
            followed.change = Change::Unknown;
            followed.loop_wide = None;
        }
    }
}
