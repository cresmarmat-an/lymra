/*!
 * Small programs built from every combination of a few parts, each aimed at
 * one kind of bound the analysis works out. Their literals put a value
 * exactly on an end of its type, or one past it, so an off-by-one in the
 * analysis shows as an unchecked overflow on some input.
 */

use lymra::ir::IntegerType;

use super::generator::{PROBE, literal_text};

/// The 8-bit types, whose every value a probe can be run on.
const TYPES: [IntegerType; 2] = [IntegerType::Integer8, IntegerType::Unsigned8];

/// Each comparison with the one that means the same when its sides are swapped.
const COMPARISONS: [(&str, &str); 6] = [
    ("<", ">"),
    ("<=", ">="),
    (">", "<"),
    (">=", "<="),
    ("==", "=="),
    ("!=", "!="),
];

/// A program with a `probe` function to run on every value of `ty`.
pub struct Probe {
    pub text: String,
    pub ty: IntegerType,
}

/// Values at and next to each end of the type, and around zero.
fn edges(ty: IntegerType) -> Vec<i128> {
    let (minimum, maximum) = (ty.minimum(), ty.maximum());
    let mut values = vec![
        minimum,
        minimum + 1,
        minimum + 2,
        -1,
        0,
        1,
        maximum / 2,
        maximum - 2,
        maximum - 1,
        maximum,
    ];
    values.retain(|&value| ty.contains(value));
    values.sort_unstable();
    values.dedup();
    values
}

/// Positive amounts that take `bound` to an end of the type, or one or two past it, and the smallest steps.
fn amounts(ty: IntegerType, bound: i128) -> Vec<i128> {
    let distances = [ty.maximum() - bound, bound - ty.minimum()];
    let mut amounts: Vec<i128> = distances
        .iter()
        .flat_map(|&distance| [distance, distance + 1, distance + 2])
        .chain([1, 2])
        .filter(|&amount| amount > 0 && ty.contains(amount))
        .collect();
    amounts.sort_unstable();
    amounts.dedup();
    amounts
}

/**
 * Compares the input with a literal, plainly, with the sides swapped, or
 * under `not`, then adds or subtracts an amount in both branches. The amount
 * takes the literal to an end of the type, give or take one, which is where a
 * bound narrowed one too far would remove a check that can fail.
 */
pub fn narrowing() -> Vec<Probe> {
    let mut probes = Vec::new();
    for ty in TYPES {
        for bound in edges(ty) {
            let bound_text = literal_text(bound);
            for (operator, mirrored) in COMPARISONS {
                let conditions = [
                    format!("input {operator} {bound_text}"),
                    format!("{bound_text} {mirrored} input"),
                    format!("not (input {operator} {bound_text})"),
                ];
                for condition in &conditions {
                    for arithmetic in ["+", "-"] {
                        for amount in amounts(ty, bound) {
                            probes.push(Probe {
                                text: narrowing_probe(ty, condition, arithmetic, amount),
                                ty,
                            });
                        }
                    }
                }
            }
        }
    }
    probes
}

fn narrowing_probe(ty: IntegerType, condition: &str, arithmetic: &str, amount: i128) -> String {
    let ty = ty.name();
    format!(
        "\
function {PROBE}({ty} input)
    variable {ty} result = 0
    if {condition}
        result = input {arithmetic} {amount}
    else
        result = input {arithmetic} {amount}
    print(\"{{result}}\")

function main()
    {PROBE}(0)
"
    )
}

/// How a loop in `accumulation` changes its total.
#[derive(Clone, Copy)]
enum LoopShape {
    /// Every iteration adds the step.
    Plain,
    /// Every iteration adds the step in a branch that ends with `continue`; the line after it adds only 1.
    Continue,
    /// Every iteration adds 1, and the last adds the step too, in a branch that ends with `break`.
    Break,
}

/**
 * Adds a step to a total in a `for` loop, or subtracts it, from a start that
 * the most the loop can do takes exactly to an end of the type, or one past
 * it. Each program's `main` does all the work.
 */
pub fn accumulation() -> Vec<String> {
    let mut programs = Vec::new();
    for ty in TYPES {
        for trips in [1, 2, 3, 5, 10] {
            for step in [1, 2, 3, 7] {
                for shape in [LoopShape::Plain, LoopShape::Continue, LoopShape::Break] {
                    let most = match shape {
                        LoopShape::Plain | LoopShape::Continue => trips * step,
                        LoopShape::Break => trips + step,
                    };
                    for past_the_end in [0, 1] {
                        for arithmetic in ["+", "-"] {
                            let start = match arithmetic {
                                "+" => ty.maximum() - most + past_the_end,
                                _ => ty.minimum() + most - past_the_end,
                            };
                            if ty.contains(start) {
                                programs.push(accumulation_program(
                                    ty, start, trips, step, shape, arithmetic,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    programs
}

fn accumulation_program(
    ty: IntegerType,
    start: i128,
    trips: i128,
    step: i128,
    shape: LoopShape,
    arithmetic: &str,
) -> String {
    let name = ty.name();
    let body = match shape {
        LoopShape::Plain => format!(
            "    for _ in range(to_{name}(0), {trips})\n        total = total {arithmetic} {step}\n"
        ),
        LoopShape::Continue => format!(
            "    for index in range(to_{name}(0), {trips})\n        if index < {trips}\n            total = total {arithmetic} {step}\n            continue\n        total = total {arithmetic} 1\n"
        ),
        LoopShape::Break => format!(
            "    for index in range(to_{name}(0), {trips})\n        total = total {arithmetic} 1\n        if index == {}\n            total = total {arithmetic} {step}\n            break\n",
            trips - 1
        ),
    };
    format!(
        "function main()\n    variable {name} total = {}\n{body}    print(\"{{total}}\")\n",
        literal_text(start)
    )
}

/**
 * Counts up or down once for each value below the input, from a start that
 * the largest input takes exactly to an end of the type, or one past it. The
 * trip count is only known from the input's bounds.
 */
pub fn counted_by_input() -> Vec<Probe> {
    let mut probes = Vec::new();
    for ty in TYPES {
        // `range(0, input)` runs at most `maximum` times.
        let most = ty.maximum();
        for past_the_end in [0, 1] {
            for (arithmetic, start) in [
                ("+", ty.maximum() - most + past_the_end),
                ("-", ty.minimum() + most - past_the_end),
            ] {
                let name = ty.name();
                let text = format!(
                    "\
function {PROBE}({name} input)
    variable {name} total = {}
    for _ in range(to_{name}(0), input)
        total = total {arithmetic} 1
    print(\"{{total}}\")

function main()
    {PROBE}(0)
",
                    literal_text(start)
                );
                probes.push(Probe { text, ty });
            }
        }
    }
    probes
}
