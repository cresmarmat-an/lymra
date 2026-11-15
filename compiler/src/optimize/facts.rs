/*!
 * What is known across calls: the values each function's parameters can
 * have, joined over every call in the program, and the values it can return,
 * joined over every `return` it reaches. A program is one file whose only
 * entry is `main`, so every call that can happen is a call the analysis sees.
 *
 * The two depend on each other, as an argument can be another function's
 * result, so they are worked out together, from `main` outward. A function
 * is analyzed again whenever what it relies on grows: the values its
 * parameters receive, or the values a function it calls returns. The facts
 * only grow, and a value that keeps growing is widened to its type's end, so
 * the work ends. It ends when no function waits: each was last analyzed with
 * the facts as they stand, so every call and every return lies within the
 * facts the analysis assumed.
 */

use std::collections::{BTreeSet, VecDeque};

use super::analysis::{self, WIDENING_DELAY};
use super::interval::Interval;
use crate::ir::{FunctionId, IntegerType, Program, Type, Value};

/// Growths a value can have once widening starts: its first bounds, then each end to the end of its type.
const GROWTHS_AFTER_DELAY: usize = 3;

/// The values a parameter or a result can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Seen {
    /// No call or `return` has reached it yet.
    Nothing,
    Within(Interval),
    /// Any value of its type, or a value the analysis does not follow, such as one that is not an integer.
    Anything,
}

impl Seen {
    /// The values seen so far, and also `value`, where `None` stands for any value.
    fn add(self, value: Option<Interval>) -> Seen {
        match (self, value) {
            (Seen::Anything, _) | (_, None) => Seen::Anything,
            (Seen::Nothing, Some(value)) => Seen::Within(value),
            (Seen::Within(seen), Some(value)) => Seen::Within(seen.join(value)),
        }
    }

    fn join(self, other: Seen) -> Seen {
        match other {
            Seen::Nothing => self,
            Seen::Within(value) => self.add(Some(value)),
            Seen::Anything => Seen::Anything,
        }
    }

    /// `next`, where a bound that moved since `self` goes to the end of `ty`. A first value is not a move.
    fn widen(self, next: Seen, ty: Option<IntegerType>) -> Seen {
        match (self, next, ty) {
            (Seen::Within(old), Seen::Within(new), Some(ty)) => {
                let whole = Interval::of_type(ty);
                let widened = old.widen(new, whole);
                if widened == whole {
                    Seen::Anything
                } else {
                    Seen::Within(widened)
                }
            }
            (_, next, _) => next,
        }
    }

    /// The bounds the analysis may assume, `None` for any value.
    fn bounds(self) -> Option<Interval> {
        match self {
            Seen::Within(value) => Some(value),
            Seen::Nothing | Seen::Anything => None,
        }
    }
}

/// What every function receives and returns, by function id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Facts {
    /// Whether a call can reach the function at all. `main` is reached from outside.
    called: Vec<bool>,
    parameters: Vec<Vec<Seen>>,
    results: Vec<Seen>,
}

impl Facts {
    /// Facts before anything is seen: nothing is reached, passed or returned.
    fn empty(program: &Program<'_>) -> Facts {
        Facts {
            called: vec![false; program.functions.len()],
            parameters: program
                .functions
                .iter()
                .map(|function| vec![Seen::Nothing; function.parameters.len()])
                .collect(),
            results: vec![Seen::Nothing; program.functions.len()],
        }
    }

    /// Facts that assume nothing: every function may be called with any values and return any value.
    fn unknown(program: &Program<'_>) -> Facts {
        Facts {
            called: vec![true; program.functions.len()],
            parameters: program
                .functions
                .iter()
                .map(|function| vec![Seen::Anything; function.parameters.len()])
                .collect(),
            results: vec![Seen::Anything; program.functions.len()],
        }
    }

    /// Whether the analysis may assume anything about the function: it is reached, by calls it has seen.
    pub fn is_called(&self, function: FunctionId) -> bool {
        self.called.get(function).copied().unwrap_or(true)
    }

    /// The bounds of a parameter where the function starts, `None` for any value.
    pub fn parameter(&self, function: FunctionId, position: usize) -> Option<Interval> {
        self.parameters
            .get(function)
            .and_then(|parameters| parameters.get(position))
            .and_then(|seen| seen.bounds())
    }

    /// The bounds of what a call to the function returns, `None` for any value.
    pub fn result(&self, function: FunctionId) -> Option<Interval> {
        self.results.get(function).and_then(|seen| seen.bounds())
    }

    /// Records a call: the function is reached, with these argument bounds, `None` standing for any value.
    pub fn observe_call(&mut self, function: FunctionId, arguments: &[Option<Interval>]) {
        if let Some(called) = self.called.get_mut(function) {
            *called = true;
        }
        if let Some(parameters) = self.parameters.get_mut(function) {
            for (seen, argument) in parameters.iter_mut().zip(arguments) {
                *seen = seen.add(*argument);
            }
        }
    }

    /// Records a `return` of a value with these bounds, `None` standing for any value.
    pub fn observe_return(&mut self, function: FunctionId, value: Option<Interval>) {
        if let Some(seen) = self.results.get_mut(function) {
            *seen = seen.add(value);
        }
    }

    /**
     * Adds what one analysis of `function` saw, widening each value that has
     * grown too often, and returns the functions to analyze again: every
     * callee reached for the first time or passed more, and every caller of
     * `function` when it returns more.
     */
    fn absorb(
        &mut self,
        function: FunctionId,
        seen: &Facts,
        program: &Program<'_>,
        tracking: &mut Tracking,
    ) -> Vec<FunctionId> {
        let mut again = Vec::new();
        for (callee, definition) in program.functions.iter().enumerate() {
            if !seen.is_called(callee) {
                continue;
            }
            if let Some(callers) = tracking.callers.get_mut(callee) {
                callers.insert(function);
            }
            let mut grew = !self.is_called(callee);
            if let Some(called) = self.called.get_mut(callee) {
                *called = true;
            }
            for (position, &local) in definition.parameters.iter().enumerate() {
                let ty = definition
                    .locals
                    .get(local)
                    .and_then(|local| local.ty.as_integer());
                let value = seen.parameters[callee][position];
                grew |= grow(
                    &mut self.parameters[callee][position],
                    value,
                    ty,
                    &mut tracking.parameter_growths[callee][position],
                );
            }
            if grew {
                again.push(callee);
            }
        }
        let ty = program
            .functions
            .get(function)
            .and_then(|definition| definition.return_type.as_ref())
            .and_then(Type::as_integer);
        if grow(
            &mut self.results[function],
            seen.results[function],
            ty,
            &mut tracking.result_growths[function],
        ) {
            again.extend(tracking.callers[function].iter().copied());
        }
        again
    }
}

/// Joins `next` into `seen`, widened once it has grown `WIDENING_DELAY` times. Returns whether it grew.
fn grow(seen: &mut Seen, next: Seen, ty: Option<IntegerType>, growths: &mut usize) -> bool {
    let joined = seen.join(next);
    if joined == *seen {
        return false;
    }
    *seen = if *growths >= WIDENING_DELAY {
        seen.widen(joined, ty)
    } else {
        joined
    };
    *growths += 1;
    true
}

/// What the work of `program_facts` keeps besides the facts: who calls whom, and how often each value grew.
struct Tracking {
    /// For each function, the functions seen calling it.
    callers: Vec<BTreeSet<FunctionId>>,
    parameter_growths: Vec<Vec<usize>>,
    result_growths: Vec<usize>,
}

impl Tracking {
    fn new(program: &Program<'_>) -> Tracking {
        Tracking {
            callers: vec![BTreeSet::new(); program.functions.len()],
            parameter_growths: program
                .functions
                .iter()
                .map(|function| vec![0; function.parameters.len()])
                .collect(),
            result_growths: vec![0; program.functions.len()],
        }
    }
}

/// The functions waiting to be analyzed, each at most once, in the order they began waiting.
struct Waiting {
    queue: VecDeque<FunctionId>,
    queued: Vec<bool>,
}

impl Waiting {
    fn new(functions: usize) -> Waiting {
        Waiting {
            queue: VecDeque::new(),
            queued: vec![false; functions],
        }
    }

    fn push(&mut self, function: FunctionId) {
        if let Some(queued) = self.queued.get_mut(function) {
            if !*queued {
                *queued = true;
                self.queue.push_back(function);
            }
        }
    }

    fn pop(&mut self) -> Option<FunctionId> {
        let function = self.queue.pop_front()?;
        if let Some(queued) = self.queued.get_mut(function) {
            *queued = false;
        }
        Some(function)
    }
}

/**
 * The facts every function can rely on. Without `main`, nothing calls the
 * functions in a way the analysis can see, so nothing is assumed. Should the
 * rounds not settle, nothing is assumed either.
 */
pub(super) fn program_facts(program: &Program<'_>, constants: &[Value]) -> Facts {
    let Some(main) = program.main else {
        return Facts::unknown(program);
    };
    let mut facts = Facts::empty(program);
    if let Some(called) = facts.called.get_mut(main) {
        *called = true;
    }
    let mut tracking = Tracking::new(program);
    let mut waiting = Waiting::new(program.functions.len());
    waiting.push(main);
    /*
     * A function waits again only when a value grows or a function is first
     * reached, and each growth sends back at most every function. A value
     * grows at most `WIDENING_DELAY` times and then a few more, so this many
     * analyses always suffice; running out means a mistake, and nothing is
     * assumed.
     */
    let functions = program.functions.len();
    let values: usize = program
        .functions
        .iter()
        .map(|function| function.parameters.len() + 1)
        .sum();
    let growths =
        functions.saturating_add(values.saturating_mul(WIDENING_DELAY + GROWTHS_AFTER_DELAY));
    let mut analyses_left = growths.saturating_mul(functions).saturating_add(1);
    while let Some(function) = waiting.pop() {
        let Some(definition) = program.functions.get(function) else {
            continue;
        };
        analyses_left = match analyses_left.checked_sub(1) {
            Some(left) => left,
            None => return Facts::unknown(program),
        };
        let mut seen = Facts::empty(program);
        analysis::observe_function(definition, function, constants, &facts, &mut seen);
        for again in facts.absorb(function, &seen, program, &mut tracking) {
            waiting.push(again);
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn within(low: i128, high: i128) -> Seen {
        Seen::Within(Interval { low, high })
    }

    #[test]
    fn values_seen_only_grow() {
        assert_eq!(Seen::Nothing.add(Some(Interval::exact(3))), within(3, 3));
        assert_eq!(within(3, 3).add(Some(Interval::exact(7))), within(3, 7));
        assert_eq!(within(3, 7).add(None), Seen::Anything);
        assert_eq!(Seen::Anything.add(Some(Interval::exact(1))), Seen::Anything);
        assert_eq!(within(1, 2).join(Seen::Nothing), within(1, 2));
    }

    #[test]
    fn a_bound_that_moves_goes_to_the_end_of_its_type() {
        let ty = Some(IntegerType::Integer8);
        assert_eq!(within(0, 10).widen(within(0, 11), ty), within(0, 127));
        assert_eq!(within(0, 10).widen(within(-1, 10), ty), within(-128, 10));
        assert_eq!(within(0, 10).widen(within(-1, 11), ty), Seen::Anything);
        assert_eq!(within(0, 10).widen(within(0, 10), ty), within(0, 10));
        // A first value is not a move.
        assert_eq!(Seen::Nothing.widen(within(5, 6), ty), within(5, 6));
    }
}
