/*!
 * Programs where a variable takes a different value on each way through a
 * `while` loop: before it, at the end of the body, at a `continue` and at a
 * `break`. One of the values sits at an end of the type, and the input picks
 * the way, so a loop head or exit that missed a way would remove a check
 * that fails on some input. The loops also run inside another loop, which
 * probes them before the analysis settles on its own head, and inside so many
 * that the analysis runs out of walks and forgets what they change.
 */

use lymra::ir::IntegerType;

use super::generator::{PROBE, literal_text, probe_caller};
use super::grids::{Probe, TYPES};

/// A way through the loop that gives the variable its value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    Entry,
    End,
    Continue,
    Break,
}

const WAYS: [Way; 4] = [Way::Entry, Way::End, Way::Continue, Way::Break];

/// What runs around the loop under test.
#[derive(Clone, Copy)]
enum Around {
    Nothing,
    While,
    For,
    /// `while` loops that each run once, nested deeper than the analysis has walks for.
    Deep,
}

const AROUND: [Around; 4] = [Around::Nothing, Around::While, Around::For, Around::Deep];

/// How many loops `Around::Deep` puts around the loop under test.
const DEEP_NESTING: usize = 10;

/**
 * Every way paired with every surrounding loop. The way named gives the
 * value at the end of the type; the others give the value next to it. One
 * check reads the variable at the start of each iteration, the other after
 * the loop, and each adds or subtracts 1 toward that end.
 */
pub fn ways() -> Vec<Probe> {
    let mut probes = Vec::new();
    for ty in TYPES {
        for way in WAYS {
            for arithmetic in ["+", "-"] {
                for around in AROUND {
                    let text = ways_program(ty, way, arithmetic, around);
                    probes.push(Probe { text, ty });
                }
            }
        }
    }
    probes
}

fn ways_program(ty: IntegerType, edge_way: Way, arithmetic: &str, around: Around) -> String {
    let name = ty.name();
    let (edge, near) = match arithmetic {
        "+" => (ty.maximum(), ty.maximum() - 1),
        _ => (ty.minimum(), ty.minimum() + 1),
    };
    let value = |way: Way| literal_text(if way == edge_way { edge } else { near });
    // A quarter of the inputs take the `continue`, and another quarter the `break`.
    let quarter = (ty.maximum() - ty.minimum()) / 4;
    let (low, high) = (ty.minimum() + quarter, ty.maximum() - quarter);
    let mut lines = vec![format!("variable {name} level = {}", value(Way::Entry))];
    let indent = match around {
        Around::Nothing => String::new(),
        Around::While => {
            lines.push("variable integer64 round = 0".to_owned());
            lines.push("while round < 2".to_owned());
            lines.push("    round = round + 1".to_owned());
            "    ".to_owned()
        }
        Around::For => {
            lines.push("for round in range(0, 2)".to_owned());
            lines.push("    print(\"{round}\")".to_owned());
            "    ".to_owned()
        }
        Around::Deep => {
            for level in 0..DEEP_NESTING {
                let indent = "    ".repeat(level);
                lines.push(format!("{indent}variable integer64 depth_{level} = 0"));
                lines.push(format!("{indent}while depth_{level} < 1"));
                lines.push(format!("{indent}    depth_{level} = depth_{level} + 1"));
            }
            "    ".repeat(DEEP_NESTING)
        }
    };
    let inner = [
        "variable integer64 count = 0".to_owned(),
        "while count < 3".to_owned(),
        "    count = count + 1".to_owned(),
        format!("    constant {name} head = level {arithmetic} 1"),
        "    print(\"{head}\")".to_owned(),
        format!("    if input < {}", literal_text(low)),
        format!("        level = {}", value(Way::Continue)),
        "        continue".to_owned(),
        format!("    if input > {}", literal_text(high)),
        format!("        level = {}", value(Way::Break)),
        "        break".to_owned(),
        format!("    level = {}", value(Way::End)),
        format!("constant {name} after = level {arithmetic} 1"),
        "print(\"{after}\")".to_owned(),
    ];
    lines.extend(inner.iter().map(|line| format!("{indent}{line}")));
    probe_program(name, &lines, ty)
}

/**
 * A `for` loop around a `while` loop whose condition changes a variable
 * through a `variable` argument. The `for` loop cannot add up a change it
 * does not see, so after it the variable must count as changed: a check that
 * holds only for its first value would fail once the takes have run.
 */
pub fn changed_by_conditions() -> Vec<Probe> {
    TYPES
        .iter()
        .map(|&ty| {
            let name = ty.name();
            // Two takes of 1 leave `minimum`, and the last line subtracts 2 more.
            let start = literal_text(ty.minimum() + 2);
            let lines = [
                format!("variable {name} level = {start}"),
                "for round in range(0, 2)".to_owned(),
                "    variable integer64 count = 0".to_owned(),
                "    while count < 1 and take(variable level, 1) > 0".to_owned(),
                "        count = count + 1".to_owned(),
                "    print(\"{round}\")".to_owned(),
                "if input == 0".to_owned(),
                "    print(\"{level - 2}\")".to_owned(),
            ];
            Probe {
                text: format!(
                    "function take(variable {name} target, {name} amount) returns {name}\n    target = target - amount\n    return amount\n\n{}",
                    probe_program(name, &lines, ty)
                ),
                ty,
            }
        })
        .collect()
}

/// A probe function with these lines as its body, and the `main` that calls it on every input.
fn probe_program(name: &str, lines: &[String], ty: IntegerType) -> String {
    let body: String = lines.iter().map(|line| format!("    {line}\n")).collect();
    format!(
        "function {PROBE}({name} input)\n{body}\n{}",
        probe_caller(ty)
    )
}
