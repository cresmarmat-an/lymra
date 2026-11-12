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
 * - A loop is not iterated to a fixed point. Each variable it changes gets
 *   bounds that hold at every point of every iteration (see `loops.rs`), and
 *   the body is walked once more with those, to remove checks.
 *
 * Anything else is treated as any value of its type: parameters, results of
 * calls, and variables changed in ways the analysis does not follow. Its
 * mistakes can only keep a check that could have gone.
 */

use std::collections::HashSet;

use super::interval::Interval;
use super::loops::{self, Change, Changes, Followed};
use crate::ir::{
    Argument, ArithmeticOperator, Block, Branch, Call, ComparisonOperator, Expr, ExprKind,
    Function, IntegerType, Local, LocalId, LogicOperator, PrintPart, Statement, Value,
};

/**
 * How many times a loop's changes are worked out. A variable whose change
 * depends on another changed variable, as in `total = total + count`, is
 * bounded once that one is, in a later round.
 */
const REFINEMENT_ROUNDS: usize = 3;

/// Removes the checks in one function that its bounds prove unnecessary.
pub(super) fn optimize_function(function: &mut Function<'_>, constants: &[Value]) {
    let analyzer = Analyzer {
        locals: &function.locals,
        constants,
    };
    let mut bounds = Bounds(vec![None; function.locals.len()]);
    analyzer.walk_block(
        &mut function.body,
        &mut bounds,
        &mut Changes::new(),
        Mode::Remove,
    );
}

/// Bounds for each local, by id. `None` stands for any value of the local's type.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Bounds(Vec<Option<Interval>>);

impl Bounds {
    fn get(&self, local: LocalId) -> Option<Interval> {
        self.0.get(local).copied().flatten()
    }

    fn set(&mut self, local: LocalId, bounds: Option<Interval>) {
        if let Some(slot) = self.0.get_mut(local) {
            *slot = bounds;
        }
    }

    /// Bounds that hold on either of two paths.
    fn join(&self, other: &Bounds) -> Bounds {
        let joined = self
            .0
            .iter()
            .zip(&other.0)
            .map(|(first, second)| match (first, second) {
                (Some(first), Some(second)) => Some(first.join(*second)),
                _ => None,
            })
            .collect();
        Bounds(joined)
    }
}

/// Whether control can reach the statement after this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Continues,
    Leaves,
}

/// What a walk does besides tracking bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Only learn what a loop body changes. Nothing is removed.
    Probe,
    /// Remove every check the bounds prove unnecessary.
    Remove,
}

struct Analyzer<'function> {
    locals: &'function [Local<'function>],
    constants: &'function [Value],
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

impl Analyzer<'_> {
    fn integer_type(&self, local: LocalId) -> Option<IntegerType> {
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
                (Some(left), Some(right)) => left.apply(*operator, right).unwrap_or(whole_type),
                _ => whole_type,
            },
            ExprKind::Convert { operand, .. } => self.range(operand, bounds).unwrap_or(whole_type),
            ExprKind::Literal(_)
            | ExprKind::Not(_)
            | ExprKind::Compare { .. }
            | ExprKind::Logic { .. }
            | ExprKind::Call(_) => whole_type,
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

    /// Clears the checks of an expression's operations that cannot fail with values in these bounds.
    fn remove_checks(&self, expr: &mut Expr, bounds: &Bounds) {
        match &mut expr.kind {
            ExprKind::Literal(_) | ExprKind::Local(_) | ExprKind::Constant(_) => {}
            ExprKind::Negate { operand, .. }
            | ExprKind::Not(operand)
            | ExprKind::Convert { operand, .. } => {
                self.remove_checks(operand, bounds);
            }
            ExprKind::Arithmetic { left, right, .. }
            | ExprKind::Compare { left, right, .. }
            | ExprKind::Logic { left, right, .. } => {
                self.remove_checks(left, bounds);
                self.remove_checks(right, bounds);
            }
            ExprKind::Call(call) => self.remove_argument_checks(call, bounds),
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

    fn remove_argument_checks(&self, call: &mut Call, bounds: &Bounds) {
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
    fn evaluate(&self, expr: &mut Expr, bounds: &mut Bounds, changes: &mut Changes, mode: Mode) {
        if mode == Mode::Remove {
            self.remove_checks(expr, bounds);
        }
        let mut passed = HashSet::new();
        loops::collect_references(expr, &mut passed);
        forget(&passed, bounds, changes);
    }

    fn walk_block(
        &self,
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
        &self,
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
            }
            Statement::Assign { target, value } => {
                let range = self.range(value, bounds);
                let step = loops::additive_terms(value, *target)
                    .and_then(|terms| self.sum_range(&terms, bounds));
                self.evaluate(value, bounds, changes, mode);
                assign(*target, range, step, bounds, changes);
            }
            Statement::Discard(value) => self.evaluate(value, bounds, changes, mode),
            Statement::Call(call) => {
                if mode == Mode::Remove {
                    self.remove_argument_checks(call, bounds);
                }
                let mut passed = HashSet::new();
                for argument in &call.arguments {
                    match argument {
                        Argument::Reference(local) => {
                            passed.insert(*local);
                        }
                        Argument::Value(value) => loops::collect_references(value, &mut passed),
                    }
                }
                forget(&passed, bounds, changes);
            }
            Statement::Print(parts) => {
                for part in parts {
                    if let PrintPart::Value(value) = part {
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
            Statement::Assert { condition } => {
                self.evaluate(condition, bounds, changes, mode);
                // The program stops when an assertion fails, so after one its condition holds.
                self.refine(bounds, condition, true);
            }
            Statement::Return(value) => {
                if let Some(value) = value {
                    self.evaluate(value, bounds, changes, mode);
                }
                return Flow::Leaves;
            }
            Statement::Break | Statement::Continue => return Flow::Leaves,
        }
        Flow::Continues
    }

    fn walk_if(
        &self,
        branches: &mut [Branch],
        otherwise: Option<&mut Block>,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) -> Flow {
        // `remaining` holds where every condition so far was false.
        let mut remaining = bounds.clone();
        let mut after: Option<Bounds> = None;
        let mut joined_changes: Option<Changes> = None;
        let mut join = |branch_bounds: Bounds, branch_changes: Changes, flow: Flow| {
            if flow == Flow::Continues {
                after = Some(match after.take() {
                    Some(previous) => previous.join(&branch_bounds),
                    None => branch_bounds,
                });
            }
            // A branch that leaves still changed things before it left, inside the current iteration.
            joined_changes = Some(match joined_changes.take() {
                Some(previous) => loops::join_changes(&previous, &branch_changes),
                None => branch_changes,
            });
        };

        for branch in branches.iter_mut() {
            self.evaluate(&mut branch.condition, &mut remaining, changes, mode);
            let mut inside = remaining.clone();
            self.refine(&mut inside, &branch.condition, true);
            let mut branch_changes = changes.clone();
            let flow = self.walk_block(&mut branch.body, &mut inside, &mut branch_changes, mode);
            join(inside, branch_changes, flow);
            self.refine(&mut remaining, &branch.condition, false);
        }
        let every_branch_leaves = match otherwise {
            Some(block) => {
                let mut inside = remaining.clone();
                let mut branch_changes = changes.clone();
                let flow = self.walk_block(block, &mut inside, &mut branch_changes, mode);
                join(inside, branch_changes, flow);
                after.is_none()
            }
            None => {
                join(remaining.clone(), changes.clone(), Flow::Continues);
                false
            }
        };
        *bounds = after.unwrap_or(remaining);
        if let Some(joined) = joined_changes {
            *changes = joined;
        }
        if every_branch_leaves {
            Flow::Leaves
        } else {
            Flow::Continues
        }
    }

    /// A `while` loop runs an unknown number of times, so every variable it changes can hold any value.
    fn walk_while(
        &self,
        condition: &mut Expr,
        body: &mut Block,
        bounds: &mut Bounds,
        changes: &mut Changes,
        mode: Mode,
    ) {
        let mut changed = loops::changed_outside(body);
        loops::collect_references(condition, &mut changed);
        forget(&changed, bounds, changes);
        self.evaluate(condition, bounds, changes, mode);
        let mut inside = bounds.clone();
        self.refine(&mut inside, condition, true);
        self.walk_block(body, &mut inside, &mut Changes::new(), mode);
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_for(
        &self,
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
        let (Some(start_range), Some(end_range)) = (start_range, end_range) else {
            forget(&changed, bounds, changes);
            return;
        };
        let trips = loops::trip_count(start_range, end_range);
        if trips == 0 {
            // The body never runs, so it changes nothing.
            return;
        }
        let value = Interval {
            low: start_range.low,
            high: end_range.high - 1,
        };

        let rounds = if mode == Mode::Remove {
            REFINEMENT_ROUNDS
        } else {
            1
        };
        let LoopBounds {
            loop_wide,
            entry,
            extents,
        } = self.loop_bounds(body, variable, value, trips, &changed, bounds, rounds);

        // Tell an enclosing loop being probed what this whole loop does to the variables it follows.
        for local in &changed {
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
            if let Some(variable) = variable {
                inside.set(variable, Some(value));
            }
            self.walk_block(body, &mut inside, &mut Changes::new(), Mode::Remove);
        }
        // After the loop, every variable it changed is still within its loop-wide bounds.
        for &local in &changed {
            let after = match changes.get(&local) {
                Some(followed) => followed.loop_wide,
                None => loop_wide.get(local),
            };
            bounds.set(local, after);
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
        &self,
        body: &mut Block,
        variable: Option<LocalId>,
        value: Interval,
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
            if let Some(variable) = variable {
                probe.set(variable, Some(value));
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
            self.walk_block(body, &mut probe, &mut changes, Mode::Probe);

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
    fn refine(&self, bounds: &mut Bounds, condition: &Expr, outcome: bool) {
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
            }
            _ => {}
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
}

/// Forgets what is known about locals that something may have changed in an unknown way.
fn forget(locals: &HashSet<LocalId>, bounds: &mut Bounds, changes: &mut Changes) {
    for &local in locals {
        bounds.set(local, None);
        if let Some(followed) = changes.get_mut(&local) {
            followed.change = Change::Unknown;
            followed.loop_wide = None;
        }
    }
}
