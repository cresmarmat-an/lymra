/*!
 * Random programs full of the range loops an optimized build writes in
 * another shape: loops that take their counter's remainder by small
 * numbers, which run in blocks of turns, and loops that push onto a list
 * once on every turn, which reserve room for every element first. The loops
 * start at multiples of the block size and elsewhere, below zero too, and
 * end with turns left over after the last whole block. Counters have every
 * width; remainders are by divisors of the block size and by other numbers;
 * bodies hold `continue`, `break`, `return` in a function, nested loops,
 * pushes in nested loops, reads of the list being filled, and arithmetic
 * that overflows on some turn. A last loop goes over the filled list and may
 * push onto it as it goes. Each program is compiled to C with every C
 * compiler, with every check and optimized, and has to print what the
 * interpreter prints and stop at the same turn.
 */

use std::ops::AddAssign;

use super::generator::Random;
use super::{common, compiled};

/// How many programs a run writes.
const PROGRAMS: usize = 60;
const SEED: u64 = 0x424C_4F43_4B53_0001;

/// How many helper functions a program has, each a loop that returns from inside.
const HELPERS: usize = 2;
/// How many accumulators `main` folds remainders into.
const ACCUMULATORS: usize = 2;
/// The most loops `main` runs one after another.
const LOOPS: usize = 3;
/// The most statements a loop body starts with.
const STATEMENTS: usize = 3;
/// How deep loops nest.
const NESTING: usize = 2;
/// Fingerprints are kept below this, so folding one more remainder in never overflows.
const PRIME: u32 = 1_000_003;

/// Block sizes a loop leans towards: its remainders are mostly by these and by numbers that divide them.
const SIZES: [u32; 12] = [2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 16];

/// The function every program has, which adds up the elements of the list its loops fill.
const TOTAL: &str = "\
function total(view[integer64] values) returns integer64
    variable integer64 sum = 0
    for value in values
        sum = (sum * 7 + value) % 1000003
    return sum

";

/// A counter type other than `integer64`, the highest end a range of it gets, and whether it is signed.
const NARROW: [(&str, i64, bool); 6] = [
    ("integer8", 100, true),
    ("unsigned8", 200, false),
    ("integer16", 300, true),
    ("unsigned16", 300, false),
    ("integer32", 300, true),
    ("unsigned32", 300, false),
];

/// A loop counter in scope: its name, its C type's Lymra name, and the block size its remainders lean towards.
#[derive(Clone)]
struct Counter {
    name: String,
    ty: &'static str,
    size: u32,
}

struct Writer<'random> {
    random: &'random mut Random,
    text: String,
    depth: usize,
    names: usize,
    /// The counters of the loops around the current line, innermost last.
    counters: Vec<Counter>,
    /// Whether `main` has an `integer8` that remainders are added to, which overflows on some turn.
    small: bool,
}

pub fn program(random: &mut Random) -> String {
    let small = random.chance(30);
    let mut writer = Writer {
        random,
        text: String::new(),
        depth: 0,
        names: 0,
        counters: Vec::new(),
        small,
    };
    writer.text.push_str(TOTAL);
    for helper in 0..HELPERS {
        writer.helper(helper);
    }
    writer.main();
    writer.text
}

impl Writer<'_> {
    fn line(&mut self, text: &str) {
        self.text.push_str(&"    ".repeat(self.depth));
        self.text.push_str(text);
        self.text.push('\n');
    }

    fn name(&mut self, role: &str) -> String {
        self.names += 1;
        format!("{role}_{}", self.names)
    }

    fn below(&mut self, bound: u32) -> u32 {
        self.random.below(bound as usize) as u32
    }

    /// A function that returns the first index past a threshold with a given remainder, or -1.
    fn helper(&mut self, index: usize) {
        let size = *self.random.pick(&SIZES);
        let wanted = self.below(size);
        let threshold = self.below(30);
        self.line(&format!(
            "function pick_{index}(integer64 limit) returns integer64"
        ));
        self.depth += 1;
        self.line("for index in range(0, limit)");
        self.depth += 1;
        self.line(&format!(
            "if index % {size} == {wanted} and index > {threshold}"
        ));
        self.depth += 1;
        self.line("return index");
        self.depth -= 2;
        self.line("return -1");
        self.depth -= 1;
        self.line("");
    }

    fn main(&mut self) {
        self.line("function main()");
        self.depth += 1;
        for accumulator in 0..ACCUMULATORS {
            let start = self.below(1_000);
            self.line(&format!("variable integer64 sum_{accumulator} = {start}"));
        }
        if self.small {
            // Near the top of its type, so adding remainders overflows on some turn.
            let start = 60 + self.below(60);
            self.line(&format!("variable integer8 small = {start}"));
            self.line("small = small + 1");
        }
        self.line("variable list[integer64] kept = list()");
        self.line("push(variable kept, 0)");
        for _ in 0..=self.random.below(LOOPS) {
            self.range_loop(NESTING);
        }
        if self.random.chance(60) {
            self.walk_kept();
        }
        // Each accumulator gets a helper's result at the end, so each changes.
        for helper in 0..HELPERS {
            let limit = self.below(60);
            let index = helper % ACCUMULATORS;
            self.line(&format!(
                "sum_{index} = sum_{index} + pick_{helper}({limit})"
            ));
        }
        let mut shown: Vec<String> = (0..ACCUMULATORS)
            .map(|index| format!("{{sum_{index}}}"))
            .collect();
        if self.small {
            shown.push("{small}".to_owned());
        }
        shown.push("{length(kept)} {total(kept)}".to_owned());
        self.line(&format!("print(\"{}\")", shown.join(" ")));
    }

    /// A loop over the list the range loops filled, which may push onto that list as it goes over it.
    fn walk_kept(&mut self) {
        let value = self.name("value");
        self.line(&format!("for {value} in kept"));
        self.depth += 1;
        self.fold_in(&value);
        if self.random.chance(60) {
            self.line(&format!("push(variable kept, {value} % 5)"));
        }
        self.depth -= 1;
    }

    /// `for counter in range(start, end)`, with a start that is a multiple of the block size or not.
    fn range_loop(&mut self, nesting: usize) {
        let size = *self.random.pick(&SIZES);
        let (ty, highest, signed) = if self.random.chance(30) {
            *self.random.pick(&NARROW)
        } else {
            ("integer64", 1_000, true)
        };
        let start: i64 = match self.random.below(10) {
            0..=4 => 0,
            5 | 6 => i64::from(size * (1 + self.below(3))),
            7 | 8 => i64::from(1 + self.below(20)),
            _ if signed => -i64::from(1 + self.below(20)),
            _ => 0,
        };
        let end =
            (start + i64::from(self.below(46)) - 3).clamp(if signed { -30 } else { 0 }, highest);
        let name = self.name("turn");
        let start_text = if ty == "integer64" {
            start.to_string()
        } else {
            format!("{ty}({start})")
        };
        self.line(&format!("for {name} in range({start_text}, {end})"));
        let counter = Counter { name, ty, size };
        let statements = self.random.below(STATEMENTS);
        self.loop_body(counter, statements, nesting);
    }

    /// The body of a loop over `counter`: a fold of its own remainder, so it is read, then `statements` more.
    fn loop_body(&mut self, counter: Counter, statements: usize, nesting: usize) {
        self.depth += 1;
        let remainder = widened(self.remainder_of(&counter));
        self.fold_in(&remainder);
        if self.small && self.random.chance(50) {
            self.line(&format!("small = small + integer8({remainder})"));
        }
        if self.random.chance(40) {
            self.line(&format!(
                "push(variable kept, {remainder} * 3 + length(kept))"
            ));
        }
        self.counters.push(counter);
        for _ in 0..statements {
            self.statement(nesting);
        }
        self.counters.pop();
        self.depth -= 1;
    }

    /// The remainder of a counter in scope, mostly by a number that divides its block size.
    fn remainder(&mut self) -> (String, &'static str, u32) {
        let counter = self.random.pick(&self.counters).clone();
        self.remainder_of(&counter)
    }

    /// The remainder of `counter`, mostly by a number that divides its block size, with its type and the divisor.
    fn remainder_of(&mut self, counter: &Counter) -> (String, &'static str, u32) {
        let divisor = if self.random.chance(85) {
            let divisors: Vec<u32> = (2..=counter.size)
                .filter(|divisor| counter.size % divisor == 0)
                .collect();
            *self.random.pick(&divisors)
        } else {
            2 + self.below(19)
        };
        (format!("{} % {divisor}", counter.name), counter.ty, divisor)
    }

    /// A remainder of a counter in scope as an `integer64`.
    fn wide_remainder(&mut self) -> String {
        widened(self.remainder())
    }

    fn statement(&mut self, nesting: usize) {
        match self.random.below(10) {
            0..=2 => self.fold(),
            3 => self.show(),
            4 if self.small => {
                let remainder = self.wide_remainder();
                self.line(&format!("small = small + integer8({remainder})"));
            }
            5 | 6 => self.branch(nesting),
            7 if nesting > 0 => self.nested_loop(nesting - 1),
            8 => self.call(),
            9 if self.random.chance(50) => {
                let index = self.random.below(ACCUMULATORS);
                self.line(&format!(
                    "sum_{index} = (sum_{index} + kept[length(kept) - 1]) % {PRIME}"
                ));
            }
            _ => self.fold(),
        }
    }

    /// Folds a remainder of a counter in scope into a fingerprint, so a turn out of order shows.
    fn fold(&mut self) {
        let remainder = self.wide_remainder();
        self.fold_in(&remainder);
    }

    /// Folds `remainder`, an `integer64`, into a fingerprint.
    fn fold_in(&mut self, remainder: &str) {
        let index = self.random.below(ACCUMULATORS);
        let factor = 2 + self.below(35);
        self.line(&format!(
            "sum_{index} = (sum_{index} * {factor} + {remainder} + 1) % {PRIME}"
        ));
    }

    fn show(&mut self) {
        let (remainder, _, _) = self.remainder();
        let counter = self.random.pick(&self.counters).name.clone();
        self.line(&format!("print(\"{{{counter}}} {{{remainder}}}\")"));
    }

    /// An `if` on a remainder, which may leave the turn with `continue` or the loop with `break`.
    fn branch(&mut self, nesting: usize) {
        let (remainder, _, divisor) = self.remainder();
        let comparison = *self.random.pick(&["==", "!=", "<"]);
        // Never `< 0`, which no remainder of a counter that never goes below zero is.
        let wanted = match comparison {
            "<" => 1 + self.below(divisor - 1),
            _ => self.below(divisor),
        };
        self.line(&format!("if {remainder} {comparison} {wanted}"));
        self.depth += 1;
        match self.random.below(10) {
            0 | 1 => {
                self.fold();
                self.line("continue");
            }
            2 => {
                self.show();
                self.line("break");
            }
            _ => self.statement(nesting),
        }
        self.depth -= 1;
        if self.random.chance(30) {
            self.line("else");
            self.depth += 1;
            self.statement(nesting);
            self.depth -= 1;
        }
    }

    /// A loop inside the body, which may run as many turns as a remainder of the outer counter says.
    fn nested_loop(&mut self, nesting: usize) {
        if self.random.chance(50) {
            let outer = self.random.pick(&self.counters).clone();
            let size = *self.random.pick(&SIZES);
            let name = self.name("inner");
            let end = if outer.ty == "integer64" {
                format!("{} % {} + 1", outer.name, outer.size)
            } else {
                format!("integer64({} % {}) + 1", outer.name, outer.size)
            };
            self.line(&format!("for {name} in range(0, {end})"));
            let counter = Counter {
                name,
                ty: "integer64",
                size,
            };
            self.loop_body(counter, 1, nesting);
        } else {
            self.range_loop(nesting);
        }
    }

    fn call(&mut self) {
        let helper = self.random.below(HELPERS);
        let remainder = self.wide_remainder();
        self.line(&format!(
            "sum_1 = (sum_1 + pick_{helper}({remainder} * 5)) % {PRIME}"
        ));
    }
}

/// `remainder`, of type `ty`, as an `integer64`.
fn widened((remainder, ty, _): (String, &'static str, u32)) -> String {
    if ty == "integer64" {
        remainder
    } else {
        format!("integer64({remainder})")
    }
}

/// How many loops of the optimized C run in each other shape.
#[derive(Clone, Copy, Default)]
struct Shapes {
    in_blocks: usize,
    reserved: usize,
}

impl Shapes {
    fn of(c_source: &str) -> Self {
        let count = |found: fn(&str) -> bool| {
            c_source
                .lines()
                .map(str::trim_start)
                .filter(|line| found(line))
                .count()
        };
        Self {
            in_blocks: count(|line| line.starts_with("for (") && line.contains("block_place")),
            reserved: count(|line| line.starts_with("if (") && line.contains("_reserve(")),
        }
    }
}

impl AddAssign for Shapes {
    fn add_assign(&mut self, other: Self) {
        self.in_blocks += other.in_blocks;
        self.reserved += other.reserved;
    }
}

#[test]
fn loops_in_other_shapes_print_what_the_interpreter_prints() {
    let compilers = common::c_compilers();
    let mut random = Random::new(SEED);
    let texts: Vec<String> = (0..PROGRAMS).map(|_| program(&mut random)).collect();
    let tally = compiled::run_all(&texts, "loop_shapes.lym", &compilers, |_, _, c_source| {
        Shapes::of(c_source)
    });
    assert!(
        tally.agreed * 3 >= PROGRAMS * 2,
        "only {} of {PROGRAMS} loop shape programs ran; {} ran out of steps",
        tally.agreed,
        tally.too_long
    );
    let Shapes {
        in_blocks,
        reserved,
    } = tally.measured;
    assert!(
        in_blocks >= PROGRAMS && reserved >= PROGRAMS / 2,
        "only {in_blocks} loops ran in blocks of turns and {reserved} reserved room"
    );
    println!(
        "{} of {PROGRAMS} loop shape programs agreed, {} ran out of steps, {in_blocks} loops ran in blocks, {reserved} reserved room",
        tally.agreed, tally.too_long
    );
}
