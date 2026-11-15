/*!
 * Programs whose checks depend on what is known across calls: a parameter
 * holds what every call passes it, and a call gives what every `return`
 * gives. Each program puts one of those at an end of the type, give or take
 * one, so a fact that missed a call, a `return` or a level of recursion
 * would remove a check that fails on some input.
 */

use lymra::ir::IntegerType;

use super::generator::{PROBE, literal_text, probe_caller};
use super::grids::{COMPARISONS, Probe, TYPES, amounts, edges};

/**
 * A function called from both branches of an `if`: with the input, which
 * the condition narrows, and with 0. It adds an amount to its parameter or
 * subtracts it, so its check may go only if it is safe for every call.
 */
pub fn parameters() -> Vec<Probe> {
    each_condition(|ty, condition, arithmetic, amount| {
        let name = ty.name();
        format!(
            "\
function shifted({name} value) returns {name}
    return value {arithmetic} {amount}

function {PROBE}({name} input)
    variable {name} result = 0
    if {condition}
        result = shifted(input)
    else
        result = shifted(0)
    print(\"{{result}}\")

{}",
            probe_caller(ty)
        )
    })
}

/**
 * A function that returns its parameter where a condition holds and 0
 * elsewhere. The caller adds an amount to the result or subtracts it, so
 * that check may go only if it is safe for every `return`.
 */
pub fn results() -> Vec<Probe> {
    each_condition(|ty, condition, arithmetic, amount| {
        let name = ty.name();
        let condition = condition.replace("input", "value");
        format!(
            "\
function capped({name} value) returns {name}
    if {condition}
        return value
    return 0

function {PROBE}({name} input)
    constant {name} result = capped(input) {arithmetic} {amount}
    print(\"{{result}}\")

{}",
            probe_caller(ty)
        )
    })
}

/// Calls `write` with each comparison of the input with an edge of the type, and each amount that takes it to an end.
fn each_condition(write: impl Fn(IntegerType, &str, &str, i128) -> String) -> Vec<Probe> {
    let mut probes = Vec::new();
    for ty in TYPES {
        for bound in edges(ty) {
            for (operator, _) in COMPARISONS {
                let condition = format!("input {operator} {}", literal_text(bound));
                for arithmetic in ["+", "-"] {
                    for amount in amounts(ty, bound) {
                        let text = write(ty, &condition, arithmetic, amount);
                        probes.push(Probe { text, ty });
                    }
                }
            }
        }
    }
    probes
}

/// Where a recursive function moves a value one step at each level.
#[derive(Clone, Copy)]
enum Steps {
    /// The amount it passes down.
    Parameter,
    /// The result it passes up.
    Result,
}

/**
 * A function that calls itself as deep as the input says, up to a limit,
 * and moves a value one step at each level, from a start that the deepest
 * call takes exactly to an end of the type, or one past it. What the
 * analysis knows has to grow with every level, as many as there are.
 */
pub fn recursion() -> Vec<Probe> {
    let mut probes = Vec::new();
    for ty in TYPES {
        for deepest in [1, 2, 3, 5, 8] {
            for step in [1, 3] {
                for steps in [Steps::Parameter, Steps::Result] {
                    for past_the_end in [0, 1] {
                        for arithmetic in ["+", "-"] {
                            let most = deepest * step;
                            let start = match arithmetic {
                                "+" => ty.maximum() - most + past_the_end,
                                _ => ty.minimum() + most - past_the_end,
                            };
                            let text =
                                recursion_program(ty, deepest, step, steps, arithmetic, start);
                            probes.push(Probe { text, ty });
                        }
                    }
                }
            }
        }
    }
    probes
}

fn recursion_program(
    ty: IntegerType,
    deepest: i128,
    step: i128,
    steps: Steps,
    arithmetic: &str,
    start: i128,
) -> String {
    let name = ty.name();
    let recursion = match steps {
        Steps::Parameter => format!("descend(depth - 1, amount {arithmetic} {step})"),
        Steps::Result => format!("descend(depth - 1, amount) {arithmetic} {step}"),
    };
    format!(
        "\
function descend({name} depth, {name} amount) returns {name}
    if depth <= 0 or depth > {deepest}
        return amount
    return {recursion}

function {PROBE}({name} input)
    constant {name} result = descend(input, {})
    print(\"{{result}}\")

{}",
        literal_text(start),
        probe_caller(ty)
    )
}
