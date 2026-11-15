/*!
 * Writes random programs in the part of Lymra the optimizer reasons about:
 * integer variables, arithmetic, an array of them, `if`, `for` and `while`
 * loops, calls that change a variable through a `variable` argument, and
 * calls of functions that return a value, one of them recursive.
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

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'items, T>(&mut self, items: &'items [T]) -> &'items T {
        &items[self.below(items.len())]
    }
}

/// A program whose `main` does all the work. Its variables start at edges of a random integer type.
pub fn program(random: &mut Random) -> String {
    let ty = *random.pick(&IntegerType::ALL);
    let mut writer = Writer::new(random, ty);
    writer.line("function main()");
    writer.body(&[]);
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
    writer.body(&["input"]);
    (writer.finish() + &probe_caller(ty), ty)
}

/**
 * The `main` of a probe program, which calls `probe` with every value of
 * `ty`, as the test does. The optimizer learns what a parameter holds from
 * the calls that pass it, so a `main` that passed fewer values would let it
 * remove checks that the test's other inputs reach.
 */
pub fn probe_caller(ty: IntegerType) -> String {
    format!(
        "function main()\n    for value in range({}, {})\n        {PROBE}({}(value))\n",
        literal_text(ty.minimum()),
        ty.maximum() + 1,
        ty.name()
    )
}

pub fn literal_text(value: i128) -> String {
    if value < 0 {
        format!("({value})")
    } else {
        value.to_string()
    }
}

/// The array every function body has, of the program's type.
const ARRAY: &str = "cells";

/// The recursive function, which calls itself until its depth runs out.
const DESCEND: &str = "descend";

/// The deepest `descend` goes, so a run stays well inside the interpreter's stack.
const DEEPEST: usize = 8;

struct Writer {
    random: Random,
    ty: IntegerType,
    /// The length of the array the current body declares.
    length: usize,
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
    /// The functions of two values of the program's type that the current body can call.
    helpers: Vec<String>,
    /// Whether the current body returns a value, so its branches can end with `return`.
    returns: bool,
}

impl Writer {
    fn new(random: &mut Random, ty: IntegerType) -> Self {
        let mut writer = Self {
            random: Random::new(random.next()),
            ty,
            length: 1,
            variables: Vec::new(),
            readable: Vec::new(),
            bounds: Vec::new(),
            source: String::new(),
            depth: 0,
            loop_depth: 0,
            loops_written: 0,
            helpers: Vec::new(),
            returns: false,
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
        if writer.random.chance(50) {
            writer.descend();
        }
        let helpers = writer.random.below(3);
        for index in 0..helpers {
            writer.helper(index);
        }
        writer
    }

    /**
     * A function that calls itself with one less depth until the depth runs
     * out, changing its amount on the way down or its result on the way up.
     * The analysis has to work out what both parameters and the result can
     * hold from every call, its own among them.
     */
    fn descend(&mut self) {
        let name = self.ty.name();
        let deepest = self.random.below(DEEPEST) + 1;
        self.line(&format!(
            "function {DESCEND}({name} depth, {name} amount) returns {name}"
        ));
        self.line(&format!("    if depth <= 0 or depth > {deepest}"));
        self.line("        return amount");
        let operator = *self.random.pick(&["+", "-", "*", "/", "%"]);
        let operand = match self.random.below(3) {
            0 => "depth".to_owned(),
            1 => "amount".to_owned(),
            _ => self.literal(),
        };
        let step = format!("amount {operator} {operand}");
        let recursion = format!("{DESCEND}(depth - 1, {step})");
        let result = match self.random.below(3) {
            0 => recursion,
            1 => format!("{} + {recursion}", self.literal()),
            _ => format!("{recursion} - depth"),
        };
        self.line(&format!("    return {result}"));
        self.line("");
        self.helpers.push(DESCEND.to_owned());
    }

    /**
     * A function of two values that returns one. Its body is written like
     * any other, with `return` ending some of its branches, and it can call
     * the functions written before it.
     */
    fn helper(&mut self, index: usize) {
        let name = format!("helper_{index}");
        let ty = self.ty.name();
        self.line(&format!(
            "function {name}({ty} first, {ty} second) returns {ty}"
        ));
        self.returns = true;
        self.body(&["first", "second"]);
        self.returns = false;
        self.helpers.push(name);
    }

    fn finish(self) -> String {
        self.source
    }

    fn line(&mut self, text: &str) {
        let _ = writeln!(self.source, "{}{text}", "    ".repeat(self.depth));
    }

    /**
     * A function body: variables, random statements, then lines that change
     * and print each variable. A body that returns a value ends with a
     * `return`; any other calls every function it can, so each is called.
     */
    fn body(&mut self, inputs: &[&str]) {
        self.depth = 1;
        self.readable
            .extend(inputs.iter().map(|&input| input.to_owned()));
        let count = self.random.below(3) + 2;
        for index in 0..count {
            let name = format!("value_{index}");
            // The first variables start as the inputs, so they can hold any value an input can.
            let initial = match inputs.get(index) {
                Some(&input) => input.to_owned(),
                None => self.literal(),
            };
            self.line(&format!("variable {} {name} = {initial}", self.ty.name()));
            self.variables.push(name);
        }
        // Short arrays put more of the indexes the program computes right at an end.
        self.length = 1 + self.random.below(12);
        let filler = self.literal();
        self.line(&format!(
            "variable array[{}, {}] {ARRAY} = array({filler})",
            self.ty.name(),
            self.length
        ));
        self.statements(2, 7);
        // Every variable has to change and be read, or the checker rejects the program.
        for name in self.variables.clone() {
            self.line(&format!("nudge(variable {name}, 0)"));
            self.line(&format!("print(\"{{{name}}}\")"));
        }
        self.line(&format!("{ARRAY}[0] = {ARRAY}[length({ARRAY}) - 1]"));
        self.line(&format!("for element in {ARRAY}"));
        self.line("    print(\"{element}\")");
        if self.returns {
            let value = self.expression(2);
            self.line(&format!("return {value}"));
        } else {
            for helper in self.helpers.clone() {
                let (first, second) = (self.operand(), self.operand());
                self.line(&format!("print(\"{{{helper}({first}, {second})}}\")"));
            }
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
        match self.random.below(13) {
            0..=3 => self.random.pick(&self.variables).clone(),
            4..=6 if !self.readable.is_empty() => self.random.pick(&self.readable).clone(),
            7 | 8 => {
                let index = self.index();
                format!("{ARRAY}[{index}]")
            }
            9 if !self.helpers.is_empty() => {
                let helper = self.random.pick(&self.helpers).clone();
                let (first, second) = (self.operand(), self.operand());
                format!("{helper}({first}, {second})")
            }
            _ => self.literal(),
        }
    }

    /**
     * An index into the array: a value the program computed, which can fall
     * outside it, or a literal inside it. A literal outside would be an error
     * while compiling.
     */
    fn index(&mut self) -> String {
        match self.random.below(6) {
            0 | 1 if !self.readable.is_empty() => self.random.pick(&self.readable).clone(),
            2 => {
                let name = self.random.pick(&self.variables).clone();
                format!("{name} % {}", self.length)
            }
            3 => self.random.below(self.length).to_string(),
            _ => self.random.pick(&self.variables).clone(),
        }
    }

    fn expression(&mut self, depth: usize) -> String {
        if depth == 0 || self.random.chance(35) {
            return self.operand();
        }
        let left = self.expression(depth - 1);
        let right = self.expression(depth - 1);
        if self.random.chance(10) {
            let function =
                *self
                    .random
                    .pick(&["wrapping_add", "wrapping_subtract", "wrapping_multiply"]);
            return format!("{function}({left}, {right})");
        }
        let operator = *self.random.pick(&["+", "-", "*", "/", "%", "+", "-"]);
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
        match self.random.below(15) {
            4 | 5 if can_nest => self.if_statement(),
            6 | 7 if can_nest => self.for_loop(),
            8 if can_nest => self.while_loop(),
            9 => self.nudge(),
            10 => self.take(),
            11 | 12 => self.element_assignment(),
            13 if can_nest => self.for_each(),
            _ => self.assignment(),
        }
    }

    fn element_assignment(&mut self) {
        let index = self.index();
        let value = self.expression(2);
        self.line(&format!("{ARRAY}[{index}] = {value}"));
    }

    /// A loop over the array. Its body can change the elements it has yet to reach.
    fn for_each(&mut self) {
        let name = format!("element_{}", self.loops_written);
        self.loops_written += 1;
        self.line(&format!("for {name} in {ARRAY}"));
        self.depth += 1;
        // Read the element first, so it is never unused.
        let target = self.random.pick(&self.variables).clone();
        self.line(&format!("{target} = {target} + {name}"));
        self.readable.push(name);
        self.loop_depth += 1;
        self.statements(0, 2);
        self.loop_depth -= 1;
        self.readable.pop();
        self.depth -= 1;
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
        let ty = self.ty.name();
        let (start, end) = match self.random.below(5) {
            0 => (self.literal(), self.literal()),
            1 => ("0".to_owned(), self.operand()),
            // A loop over the array's indexes, whose bounds checks the optimizer should prove.
            2 => ("0".to_owned(), format!("{ty}(length({ARRAY}))")),
            _ => ("0".to_owned(), (self.random.below(40) + 1).to_string()),
        };
        // The conversion gives the range the program's type; two plain literals would make it `integer64`.
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

    /**
     * A `while` loop that a counter keeps short. Its condition tests the
     * counter, the program's variables, or both; a loop on the variables
     * alone leaves through a `break` once the counter runs out.
     */
    fn while_loop(&mut self) {
        let counter = format!("count_{}", self.loops_written);
        self.loops_written += 1;
        let limit = self.random.below(30) + 1;
        self.line(&format!("variable integer64 {counter} = 0"));
        let condition = match self.random.below(3) {
            0 => format!("{counter} < {limit}"),
            1 => format!("{counter} < {limit} and ({})", self.condition()),
            _ => self.condition(),
        };
        self.line(&format!("while {condition}"));
        self.depth += 1;
        self.line(&format!("{counter} = {counter} + 1"));
        self.line(&format!("if {counter} > {limit}"));
        self.line("    break");
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
        } else if self.returns && self.random.chance(15) {
            let value = self.expression(1);
            self.line(&format!("return {value}"));
        }
        self.depth -= 1;
    }
}
