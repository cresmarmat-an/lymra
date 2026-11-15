/*!
 * What is known across calls: the values each function's parameters can
 * have, joined over every call in the program, and the values it can return,
 * joined over every `return` it reaches. A program is one file whose only
 * entry is `main`, so every call that can happen is a call the analysis sees.
 *
 * The two depend on each other, as an argument can be another function's
 * result, so they are worked out together. Each round analyzes every
 * function that something calls, with the facts of the round before, and
 * records what its calls pass and what it returns. The facts only grow, a
 * bound that keeps moving is widened to its type's end, and the rounds stop
 * when nothing grows: then every call and every return lies within the facts
 * the analysis assumed.
 */

use super::analysis::{self, WIDENING_DELAY};
use super::interval::Interval;
use crate::ir::{FunctionId, IntegerType, Program, Type, Value};

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
    /// Facts before anything is seen: only `main` is reached, and nothing is known to be passed or returned.
    fn nothing(program: &Program<'_>, main: FunctionId) -> Facts {
        let mut facts = Facts {
            called: vec![false; program.functions.len()],
            parameters: program
                .functions
                .iter()
                .map(|function| vec![Seen::Nothing; function.parameters.len()])
                .collect(),
            results: vec![Seen::Nothing; program.functions.len()],
        };
        if let Some(called) = facts.called.get_mut(main) {
            *called = true;
        }
        facts
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

    fn join(&self, other: &Facts) -> Facts {
        Facts {
            called: self
                .called
                .iter()
                .zip(&other.called)
                .map(|(first, second)| *first || *second)
                .collect(),
            parameters: self
                .parameters
                .iter()
                .zip(&other.parameters)
                .map(|(first, second)| {
                    first
                        .iter()
                        .zip(second)
                        .map(|(first, second)| first.join(*second))
                        .collect()
                })
                .collect(),
            results: self
                .results
                .iter()
                .zip(&other.results)
                .map(|(first, second)| first.join(*second))
                .collect(),
        }
    }

    /// `next`, with every bound that moved since `self` widened to the end of its type.
    fn widen(&self, next: Facts, program: &Program<'_>) -> Facts {
        let parameters = self
            .parameters
            .iter()
            .zip(next.parameters)
            .zip(&program.functions)
            .map(|((old, new), function)| {
                old.iter()
                    .zip(new)
                    .zip(&function.parameters)
                    .map(|((old, new), &local)| {
                        old.widen(
                            new,
                            function
                                .locals
                                .get(local)
                                .and_then(|local| local.ty.as_integer()),
                        )
                    })
                    .collect()
            })
            .collect();
        let results = self
            .results
            .iter()
            .zip(next.results)
            .zip(&program.functions)
            .map(|((old, new), function)| {
                old.widen(
                    new,
                    function.return_type.as_ref().and_then(Type::as_integer),
                )
            })
            .collect();
        Facts {
            called: next.called,
            parameters,
            results,
        }
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
    let mut facts = Facts::nothing(program, main);
    /*
     * Each round that does not settle reaches one more function or changes
     * one more value. Once widening starts, a value changes at most three
     * times: to its first bounds, then each end to the end of its type, the
     * second of which makes it any value.
     */
    let values: usize = program
        .functions
        .iter()
        .map(|function| function.parameters.len() + 1)
        .sum();
    let most_rounds = WIDENING_DELAY + program.functions.len() + 3 * values + 1;
    for round in 0..most_rounds {
        let mut observed = Facts::nothing(program, main);
        for (id, function) in program.functions.iter().enumerate() {
            if facts.is_called(id) {
                analysis::observe_function(function, id, constants, &facts, &mut observed);
            }
        }
        let joined = facts.join(&observed);
        let next = if round >= WIDENING_DELAY {
            facts.widen(joined, program)
        } else {
            joined
        };
        if next == facts {
            return facts;
        }
        facts = next;
    }
    Facts::unknown(program)
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
