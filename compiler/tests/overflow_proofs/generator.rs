/*!
 * Writes random programs in the part of Lymra the optimizer reasons about:
 * integer variables, arithmetic, `if`, `for` and `while` loops, and calls that
 * change a variable through a `variable` argument.
 *
 * Literals cluster at the edges of the program's type, and some are picked to
 * land exactly on, or one past, a bound that an earlier condition set. Those
 * are the values where an off-by-one in the analysis would show.
 */

use std::fmt::Write as _;

use lymra::ir::IntegerType;

/// The function a probe program runs on every input.
pub const PROBE: &str = "probe";

/// A small xorshift generator, so every run writes the same programs.
pub struct Random(u64);

impl Random {
    pub fn new(seed: u64) -> Self {
        // Xorshift never leaves zero, so the state must not start there.
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<'items, T>(&mut self, items: &'items [T]) -> &'items T {
        &items[self.below(items.len())]
    }
}

/// A program whose `main` does all the work. Its variables start at edges of a random integer type.
pub fn program(random: &mut Random) -> String {
    let ty = *random.pick(&IntegerType::ALL);
    let mut writer = Writer::new(random, ty);
    writer.line("function main()");
    writer.body(None);
    writer.finish()
}

/**
 * A program around `probe`, a function of one input of an 8-bit type. The
 * test runs it on every value of the type, so each bound the analysis works
 * out from the input gets reached exactly, and one past it as well.
 */
pub fn probe(random: &mut Random) -> (String, IntegerType) {
    let ty = *random.pick(&[IntegerType::Integer8, IntegerType::Unsigned8]);
    let mut writer = Writer::new(random, ty);
    writer.line(&format!("function {PROBE}({} input)", ty.name()));
    writer.body(Some("input"));
    writer.line("function main()");
    writer.line(&format!("    {PROBE}(0)"));
    (writer.finish(), ty)
}

pub fn literal_text(value: i128) -> String {
    if value < 0 {
        format!("({value})")
    } else {
        value.to_string()
    }
}

struct Writer {
    random: Random,
    ty: IntegerType,
    variables: Vec<String>,
    /// Names that can be read but not changed: the input and the variables of the loops around the current line.
    readable: Vec<String>,
    /// Literals that conditions compared with.
    bounds: Vec<i128>,
    source: String,
    depth: usize,
    /// How many loops are around the current line, so `break` and `continue` are only written inside one.
    loop_depth: usize,
    loops_written: usize,
}

impl Writer {
    fn new(random: &mut Random, ty: IntegerType) -> Self {
        let mut writer = Self {
            random: Random::new(random.next()),
            ty,
            variables: Vec::new(),
            readable: Vec::new(),
            bounds: Vec::new(),
            source: String::new(),
            depth: 0,
            loop_depth: 0,
            loops_written: 0,
        };
        let name = ty.name();
        writer.line(&format!(
            "function nudge(variable {name} target, {name} amount)"
        ));
        writer.line("    target = target + amount");
        writer.line("");
        writer.line(&format!(
            "function take(variable {name} target, {name} amount) returns {name}"
        ));
        writer.line("    target = target - amount");
        writer.line("    return amount");
        writer.line("");
        writer
    }

    fn finish(self) -> String {
        self.source
    }

    fn line(&mut self, text: &str) {
        let _ = writeln!(self.source, "{}{text}", "    ".repeat(self.depth));
    }

    /// A function body: variables, random statements, then a line that changes and prints each variable.
    fn body(&mut self, input: Option<&str>) {
        self.depth = 1;
        self.readable.extend(input.map(str::to_owned));
        let count = self.random.below(3) + 2;
        for index in 0..count {
            let name = format!("value_{index}");
            // In a probe, the first variable starts as the input, so it can hold any value of its type.
            let initial = match input {
                Some(input) if index == 0 => input.to_owned(),
                _ => self.literal(),
            };
            self.line(&format!("variable {} {name} = {initial}", self.ty.name()));
            self.variables.push(name);
        }
        self.statements(2, 7);
        // Every variable has to change and be read, or the checker rejects the program.
        for name in self.variables.clone() {
            self.line(&format!("nudge(variable {name}, 0)"));
            self.line(&format!("print(\"{{{name}}}\")"));
        }
        self.depth = 0;
        self.line("");
        self.variables.clear();
        self.readable.clear();
    }

    /// A literal of the program's type: usually at or near an edge of the type, sometimes just past a known bound.
    fn literal(&mut self) -> String {
        let value = self.literal_value();
        literal_text(value)
    }

    fn literal_value(&mut self) -> i128 {
        let (minimum, maximum) = (self.ty.minimum(), self.ty.maximum());
        if !self.bounds.is_empty() && self.random.chance(30) {
            let bound = *self.random.pick(&self.bounds);
            let offset = *self.random.pick(&[-1, 0, 1, 2]);
            // How far the bound is from an end of the type, give or take one.
            let distance = if self.random.chance(50) {
                maximum - bound
            } else {
                bound - minimum
            };
            if self.ty.contains(distance + offset) {
                return distance + offset;
            }
        }
        let edges = [
            0,
            1,
            2,
            3,
            7,
            -1,
            -2,
            100,
            -100,
            maximum,
            maximum - 1,
            maximum / 2,
            maximum / 3,
            minimum,
            minimum + 1,
            minimum / 2,
        ];
        (*self.random.pick(&edges)).clamp(minimum, maximum)
    }

    fn operand(&mut self) -> String {
        match self.random.below(10) {
            0..=3 => self.random.pick(&self.variables).clone(),
            4..=6 if !self.readable.is_empty() => self.random.pick(&self.readable).clone(),
            _ => self.literal(),
        }
    }

    fn expression(&mut self, depth: usize) -> String {
        if depth == 0 || self.random.chance(35) {
            return self.operand();
        }
        let operator = *self.random.pick(&["+", "-", "*", "/", "%", "+", "-"]);
        let left = self.expression(depth - 1);
        let right = self.expression(depth - 1);
        if self.ty.is_signed() && self.random.chance(10) {
            return format!("-({left} {operator} {right})");
        }
        format!("({left} {operator} {right})")
    }

    fn condition(&mut self) -> String {
        let operator = *self.random.pick(&["<", "<=", ">", ">=", "==", "!="]);
        let left = self.operand();
        let right = if self.random.chance(60) {
            let value = self.literal_value();
            self.bounds.push(value);
            literal_text(value)
        } else {
            self.expression(1)
        };
        let comparison = if self.random.chance(25) {
            format!("{right} {operator} {left}")
        } else {
            format!("{left} {operator} {right}")
        };
        match self.random.below(8) {
            0 => format!("not ({comparison})"),
            1 => format!("{comparison} and {} < {}", self.operand(), self.literal()),
            2 => format!("{comparison} or {} > {}", self.operand(), self.literal()),
            _ => comparison,
        }
    }

    /// Writes from `fewest` to `most` statements.
    fn statements(&mut self, fewest: usize, most: usize) {
        let count = fewest + self.random.below(most - fewest + 1);
        for _ in 0..count {
            self.statement();
        }
    }

    fn statement(&mut self) {
        let can_nest = self.depth < 4;
        match self.random.below(12) {
            4 | 5 if can_nest => self.if_statement(),
            6 | 7 if can_nest => self.for_loop(),
            8 if can_nest => self.while_loop(),
            9 => self.nudge(),
            10 => self.take(),
            _ => self.assignment(),
        }
    }

    fn assignment(&mut self) {
        let target = self.random.pick(&self.variables).clone();
        let value = match self.random.below(4) {
            0 => self.expression(2),
            1 => format!("{target} - {}", self.expression(1)),
            _ => format!("{target} + {}", self.expression(1)),
        };
        self.line(&format!("{target} = {value}"));
    }

    /// Changes a variable through a call, which the analysis cannot see into.
    fn nudge(&mut self) {
        let target = self.random.pick(&self.variables).clone();
        let amount = if self.readable.is_empty() || self.random.chance(60) {
            self.literal()
        } else {
            self.random.pick(&self.readable).clone()
        };
        self.line(&format!("nudge(variable {target}, {amount})"));
    }

    /// Changes a variable through a call inside an expression, as in `value_0 = 3 + take(variable value_1, 2)`.
    fn take(&mut self) {
        let target = self.random.pick(&self.variables).clone();
        let changed = self.random.pick(&self.variables).clone();
        let amount = self.literal();
        // The rest of the expression must not use `changed`, or the checker rejects it (L0508).
        let other = if self.readable.is_empty() || self.random.chance(50) {
            self.literal()
        } else {
            self.random.pick(&self.readable).clone()
        };
        self.line(&format!(
            "{target} = {other} + take(variable {changed}, {amount})"
        ));
    }

    fn if_statement(&mut self) {
        let condition = self.condition();
        self.line(&format!("if {condition}"));
        self.block(2);
        if self.random.chance(30) {
            let condition = self.condition();
            self.line(&format!("else if {condition}"));
            self.block(2);
        }
        if self.random.chance(50) {
            self.line("else");
            self.block(2);
        }
    }

    fn for_loop(&mut self) {
        let name = format!("index_{}", self.loops_written);
        self.loops_written += 1;
        let (start, end) = match self.random.below(4) {
            0 => (self.literal(), self.literal()),
            1 => ("0".to_owned(), self.operand()),
            _ => ("0".to_owned(), (self.random.below(40) + 1).to_string()),
        };
        // The conversion gives the range the program's type; two plain literals would make it `integer64`.
        let ty = self.ty.name();
        self.line(&format!("for {name} in range({ty}({start}), {end})"));
        self.depth += 1;
        // Read the loop variable first, so it is never unused.
        let target = self.random.pick(&self.variables).clone();
        self.line(&format!("{target} = {target} + {name}"));
        self.readable.push(name);
        self.loop_depth += 1;
        self.statements(0, 2);
        self.loop_depth -= 1;
        self.readable.pop();
        self.depth -= 1;
    }

    fn while_loop(&mut self) {
        let counter = format!("count_{}", self.loops_written);
        self.loops_written += 1;
        let limit = self.random.below(30) + 1;
        self.line(&format!("variable integer64 {counter} = 0"));
        self.line(&format!("while {counter} < {limit}"));
        self.depth += 1;
        self.line(&format!("{counter} = {counter} + 1"));
        self.loop_depth += 1;
        self.statements(1, 3);
        self.loop_depth -= 1;
        self.depth -= 1;
    }

    /// The body of a branch. Inside a loop, it sometimes leaves the iteration at its end.
    fn block(&mut self, most: usize) {
        self.depth += 1;
        self.statements(1, most);
        if self.loop_depth > 0 && self.random.chance(25) {
            let jump = *self.random.pick(&["break", "continue"]);
            self.line(jump);
        }
        self.depth -= 1;
    }
}
