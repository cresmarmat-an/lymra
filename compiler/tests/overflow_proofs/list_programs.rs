/*!
 * Random programs that build, grow, copy, replace and free lists in the
 * ways the language allows: in nested blocks and loops, through calls that
 * return a list or change one through `variable`, in recursion, and around
 * `break`, `continue` and `return`. They index lists in the loops and tests
 * the optimizer proves indexes by, sometimes after a change that ends the
 * proof, where the check has to stay. Each is compiled to C with every C
 * compiler, with every check and optimized, and run. It has to print what
 * the interpreter prints and end the same way. Built with the sanitizers,
 * as one CI job does, every run also shows that nothing reads a freed list
 * and that each data stack is empty when `main` returns.
 *
 * Functions only call the ones written before them, and themselves, and
 * pass a `variable` list down only as it came in. So no program puts a list
 * where a newer one sits above it (error L0704), and none loops forever.
 */

use super::generator::Random;
use super::{CheckCount, common, compiled};

/// How many programs a run writes.
const PROGRAMS: usize = 40;
const SEED: u64 = 0x4C49_5354_5300_0001;

/// How many functions a program has besides `total` and `main`.
const FUNCTIONS: usize = 5;
/// The most statements a block starts with.
const STATEMENTS: usize = 4;
/// How deep loops and branches nest.
const NESTING: usize = 3;

/// The function every program has, which adds up the elements a view shows: each list it gets becomes one.
const TOTAL: &str = "\
function total(view[integer64] values) returns integer64
    variable integer64 sum = 0
    for value in values
        sum = sum + value
    return sum

";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `(integer64 depth) returns list[integer64]`
    Builds,
    /// `(list[integer64] values, integer64 depth) returns list[integer64]`
    Maps,
    /// `(variable list[integer64] values, integer64 depth)`
    Grows,
}

struct Written {
    name: String,
    shape: Shape,
}

#[derive(Clone)]
struct List {
    name: String,
    /// Whether it can grow and be replaced: a `variable` binding or parameter.
    mutable: bool,
    /// Whether a `for` loop around the current line goes over it, so it cannot be replaced or passed with `variable`.
    iterated: bool,
}

/// What a `return` in the function being written returns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Returns {
    Nothing,
    List,
}

/// A program of list functions and a `main` that calls each of them.
pub fn program(random: &mut Random) -> String {
    let mut writer = Writer {
        random: Random::new(random.next()),
        source: String::from(TOTAL),
        depth: 0,
        written: Vec::new(),
        lists: Vec::new(),
        numbers: Vec::new(),
        loops: 0,
        names: 0,
        returns: Returns::Nothing,
    };
    for index in 0..FUNCTIONS {
        writer.function(index);
    }
    writer.main();
    writer.source
}

struct Writer {
    random: Random,
    source: String,
    depth: usize,
    /// The functions written so far, which the one being written can call.
    written: Vec<Written>,
    /// The lists visible at the current line.
    lists: Vec<List>,
    /// The integers visible at the current line.
    numbers: Vec<String>,
    /// How many loops are around the current line.
    loops: usize,
    /// Counts names, so each binding of a function has its own.
    names: usize,
    returns: Returns,
}

impl Writer {
    fn line(&mut self, text: &str) {
        self.source.push_str(&"    ".repeat(self.depth));
        self.source.push_str(text);
        self.source.push('\n');
    }

    fn name(&mut self, role: &str) -> String {
        self.names += 1;
        format!("{role}_{}", self.names)
    }

    /// A literal or a visible integer.
    fn number(&mut self) -> String {
        if !self.numbers.is_empty() && self.random.chance(50) {
            return self.random.pick(&self.numbers).clone();
        }
        self.random.below(10).to_string()
    }

    /// A depth to pass, small so recursion ends soon.
    fn depth_argument(&mut self) -> String {
        self.random.below(4).to_string()
    }

    fn start_function(&mut self, header: &str, returns: Returns) {
        self.line(header);
        self.depth = 1;
        self.lists.clear();
        self.numbers = vec!["depth".to_owned()];
        self.loops = 0;
        self.names = 0;
        self.returns = returns;
    }

    fn end_function(&mut self, name: String, shape: Shape) {
        self.depth = 0;
        self.line("");
        self.written.push(Written { name, shape });
    }

    fn function(&mut self, index: usize) {
        let name = format!("function_{index}");
        let recursive = self.random.chance(50);
        match *self
            .random
            .pick(&[Shape::Builds, Shape::Maps, Shape::Grows])
        {
            Shape::Builds => {
                self.start_function(
                    &format!("function {name}(integer64 depth) returns list[integer64]"),
                    Returns::List,
                );
                self.line("variable list[integer64] result = list()");
                self.line("push(variable result, depth)");
                self.lists.push(List {
                    name: "result".to_owned(),
                    mutable: true,
                    iterated: false,
                });
                self.statements(0);
                if recursive {
                    self.line("if depth > 0");
                    self.line(&format!("    result = {name}(depth - 1)"));
                }
                self.line("return result");
                self.end_function(name, Shape::Builds);
            }
            Shape::Maps => {
                self.start_function(
                    &format!(
                        "function {name}(list[integer64] values, integer64 depth) returns list[integer64]"
                    ),
                    Returns::List,
                );
                self.lists.push(List {
                    name: "values".to_owned(),
                    mutable: false,
                    iterated: false,
                });
                self.line("variable list[integer64] result = list()");
                self.line("for value in values");
                self.line("    push(variable result, value + depth)");
                self.lists.push(List {
                    name: "result".to_owned(),
                    mutable: true,
                    iterated: false,
                });
                self.statements(0);
                self.line("return result");
                self.end_function(name, Shape::Maps);
            }
            Shape::Grows => {
                self.start_function(
                    &format!("function {name}(variable list[integer64] values, integer64 depth)"),
                    Returns::Nothing,
                );
                self.lists.push(List {
                    name: "values".to_owned(),
                    mutable: true,
                    iterated: false,
                });
                self.line("push(variable values, depth)");
                self.statements(0);
                if recursive {
                    self.line("if depth > 0");
                    self.line(&format!("    {name}(variable values, depth - 1)"));
                }
                self.end_function(name, Shape::Grows);
            }
        }
    }

    fn main(&mut self) {
        self.start_function("function main()", Returns::Nothing);
        self.numbers.clear();
        self.declare_list("numbers", "list()".to_owned());
        self.statements(0);
        for index in 0..self.written.len() {
            let name = self.written[index].name.clone();
            let depth = self.depth_argument();
            match self.written[index].shape {
                Shape::Builds => {
                    self.line(&format!("print(\"{name} {{total({name}({depth}))}}\")"))
                }
                Shape::Maps => self.line(&format!(
                    "print(\"{name} {{total({name}(numbers, {depth}))}}\")"
                )),
                Shape::Grows => self.line(&format!("{name}(variable numbers, {depth})")),
            }
        }
        self.line("print(\"numbers {length(numbers)} {total(numbers)}\")");
        self.source.push('\n');
    }

    /// Declares a list binding with `value` and pushes to it, so it is a `variable` that changes.
    fn declare_list(&mut self, name: &str, value: String) {
        self.line(&format!("variable list[integer64] {name} = {value}"));
        let pushed = self.number();
        self.line(&format!("push(variable {name}, {pushed})"));
        self.lists.push(List {
            name: name.to_owned(),
            mutable: true,
            iterated: false,
        });
    }

    /// A block's statements, then a line that reads each list it declared.
    fn statements(&mut self, nesting: usize) {
        let lists = self.lists.len();
        let numbers = self.numbers.len();
        let start = self.source.len();
        for _ in 0..=self.random.below(STATEMENTS) {
            self.statement(nesting);
        }
        // A statement can find nothing to work on and write nothing, and a block needs a line.
        if self.source.len() == start {
            self.line("print(\"nothing to do\")");
        }
        for index in lists..self.lists.len() {
            let name = self.lists[index].name.clone();
            self.line(&format!(
                "print(\"{name} {{length({name})}} {{total({name})}}\")"
            ));
        }
        self.lists.truncate(lists);
        self.numbers.truncate(numbers);
    }

    fn statement(&mut self, nesting: usize) {
        let nested = nesting < NESTING;
        match self.random.below(15) {
            0 | 1 => {
                let name = self.name("list");
                let value = self.list_value();
                self.declare_list(&name, value);
            }
            2 | 3 => self.push(),
            4 => self.replace(),
            5 => self.index(),
            6 if nested => self.for_each(nesting),
            7 if nested => self.while_loop(nesting),
            8 => self.grow(),
            9 if nested => self.leave(),
            10 => self.condition_on_a_temporary(),
            11 if nested => self.index_loop(nesting),
            12 if nested => self.while_index(nesting),
            13 => self.guarded_index(),
            _ => self.show_a_temporary(),
        }
    }

    /// Reads or changes the element at `position` of `list`, which is `mutable` when it can be changed.
    fn use_element(&mut self, list: &List, position: &str) {
        if list.mutable && self.random.chance(50) {
            self.line(&format!("{0}[{position}] = {0}[{position}] + 1", list.name));
        } else {
            self.line(&format!(
                "print(\"at {{{position}}} {{{}[{position}]}}\")",
                list.name
            ));
        }
    }

    /// `for position in range(0, length(list))`, indexing the list in the body, perhaps after replacing it.
    fn index_loop(&mut self, nesting: usize) {
        if self.lists.is_empty() {
            return;
        }
        let list = self.random.pick(&self.lists).clone();
        let position = self.name("position");
        self.line(&format!(
            "for {position} in range(0, length({}))",
            list.name
        ));
        self.depth += 1;
        self.loops += 1;
        if list.mutable && !list.iterated && self.random.chance(25) {
            // Replacing the list ends the proof, and an index past the new end has to stop the program.
            let value = self.list_value();
            self.line(&format!("{} = {value}", list.name));
        }
        self.numbers.push(position.clone());
        if self.random.chance(50) {
            self.statements(nesting + 1);
        }
        self.use_element(&list, &position);
        self.numbers.pop();
        self.loops -= 1;
        self.depth -= 1;
    }

    /// A `while` loop that walks a list by its length.
    fn while_index(&mut self, nesting: usize) {
        if self.lists.is_empty() {
            return;
        }
        let list = self.random.pick(&self.lists).clone();
        let position = self.name("position");
        self.line(&format!("variable integer64 {position} = 0"));
        self.line(&format!("while {position} < length({})", list.name));
        self.depth += 1;
        self.loops += 1;
        self.use_element(&list, &position);
        self.line(&format!("{position} = {position} + 1"));
        self.numbers.push(position);
        if self.random.chance(30) {
            self.statements(nesting + 1);
        }
        self.loops -= 1;
        self.depth -= 1;
    }

    /// An element read where a test says the index is below the length.
    fn guarded_index(&mut self) {
        if self.lists.is_empty() || self.numbers.is_empty() {
            return;
        }
        let list = self.random.pick(&self.lists).clone();
        let position = self.random.pick(&self.numbers).clone();
        self.line(&format!("if {position} < length({})", list.name));
        self.depth += 1;
        self.use_element(&list, &position);
        self.depth -= 1;
    }

    /// A list value where its type is known: empty, a copy, or a list a call returns, which may read the list it replaces.
    fn list_value(&mut self) -> String {
        match self.random.below(4) {
            0 if !self.lists.is_empty() => self.random.pick(&self.lists).name.clone(),
            1 | 2 => self.call_value().unwrap_or_else(|| "list()".to_owned()),
            _ => "list()".to_owned(),
        }
    }

    /// A call of a function written before that returns a list, if there is one.
    fn call_value(&mut self) -> Option<String> {
        let builders: Vec<(String, Shape)> = self
            .written
            .iter()
            .filter(|function| function.shape != Shape::Grows)
            .map(|function| (function.name.clone(), function.shape))
            .collect();
        if builders.is_empty() {
            return None;
        }
        let (name, shape) = self.random.pick(&builders).clone();
        let depth = self.depth_argument();
        Some(match shape {
            Shape::Maps if !self.lists.is_empty() => {
                let list = self.random.pick(&self.lists).name.clone();
                format!("{name}({list}, {depth})")
            }
            Shape::Maps => format!("{name}(list(), {depth})"),
            Shape::Builds | Shape::Grows => format!("{name}({depth})"),
        })
    }

    /// A call that returns a list, or else a visible list.
    fn temporary_or_binding(&mut self) -> Option<String> {
        self.call_value().or_else(|| {
            (!self.lists.is_empty()).then(|| self.random.pick(&self.lists).name.clone())
        })
    }

    fn mutable_list(&mut self, iterated_too: bool) -> Option<String> {
        let candidates: Vec<String> = self
            .lists
            .iter()
            .filter(|list| list.mutable && (iterated_too || !list.iterated))
            .map(|list| list.name.clone())
            .collect();
        (!candidates.is_empty()).then(|| self.random.pick(&candidates).clone())
    }

    fn push(&mut self) {
        if let Some(list) = self.mutable_list(true) {
            let value = self.number();
            self.line(&format!("push(variable {list}, {value})"));
        }
    }

    fn replace(&mut self) {
        if let Some(list) = self.mutable_list(false) {
            let value = self.list_value();
            self.line(&format!("{list} = {value}"));
        }
    }

    fn index(&mut self) {
        if self.lists.is_empty() {
            return;
        }
        let list = self.random.pick(&self.lists).clone();
        let position = self.random.below(4);
        self.line(&format!("if length({}) > {position}", list.name));
        if list.mutable && self.random.chance(50) {
            let value = self.number();
            self.line(&format!("    {}[{position}] = {value}", list.name));
        } else {
            self.line(&format!(
                "    print(\"at {position} {{{}[{position}]}}\")",
                list.name
            ));
        }
    }

    fn for_each(&mut self, nesting: usize) {
        let item = self.name("item");
        let over_binding = !self.lists.is_empty() && self.random.chance(70);
        let (iterated, source) = if over_binding {
            let index = self.random.below(self.lists.len());
            (Some(index), self.lists[index].name.clone())
        } else {
            match self.call_value() {
                Some(call) => (None, call),
                None => return,
            }
        };
        self.line(&format!("for {item} in {source}"));
        let was_iterated = iterated.map(|index| self.lists[index].iterated);
        if let Some(index) = iterated {
            self.lists[index].iterated = true;
        }
        // The body reads the element first, so the loop variable is used.
        match self.mutable_list(true) {
            Some(list) if self.random.chance(50) => {
                self.line(&format!("    push(variable {list}, {item})"));
            }
            _ => self.line(&format!("    print(\"item {{{item}}}\")")),
        }
        self.numbers.push(item);
        self.nested(nesting);
        self.numbers.pop();
        if let (Some(index), Some(before)) = (iterated, was_iterated) {
            self.lists[index].iterated = before;
        }
    }

    fn while_loop(&mut self, nesting: usize) {
        let turn = self.name("turn");
        let turns = self.random.below(4) + 1;
        self.line(&format!("variable integer64 {turn} = 0"));
        self.line(&format!("while {turn} < {turns}"));
        // The count goes up first, so a `continue` further down cannot loop forever.
        self.line(&format!("    {turn} = {turn} + 1"));
        self.numbers.push(turn);
        self.nested(nesting);
    }

    /// The body of a loop, one level in.
    fn nested(&mut self, nesting: usize) {
        self.depth += 1;
        self.loops += 1;
        self.statements(nesting + 1);
        self.loops -= 1;
        self.depth -= 1;
    }

    /// Passes a list with `variable` to a function that grows it.
    fn grow(&mut self) {
        let growers: Vec<String> = self
            .written
            .iter()
            .filter(|function| function.shape == Shape::Grows)
            .map(|function| function.name.clone())
            .collect();
        if growers.is_empty() {
            return;
        }
        if let Some(list) = self.mutable_list(false) {
            let function = self.random.pick(&growers).clone();
            let depth = self.depth_argument();
            self.line(&format!("{function}(variable {list}, {depth})"));
        }
    }

    /// A branch that builds a list, then leaves its loop or its function with the list still open.
    fn leave(&mut self) {
        let mut exits = vec!["return"];
        if self.loops > 0 {
            exits.extend(["break", "continue"]);
        }
        let exit = *self.random.pick(&exits);
        // A test of a literal would be decided while compiling, so the test reads a list or a number that changes.
        let tested = if !self.lists.is_empty() && self.random.chance(50) {
            format!("length({})", self.random.pick(&self.lists).name)
        } else if !self.numbers.is_empty() {
            self.random.pick(&self.numbers).clone()
        } else {
            return;
        };
        let limit = self.random.below(5);
        self.line(&format!("if {tested} > {limit}"));
        self.depth += 1;
        let name = self.name("leaving");
        let value = self.list_value();
        self.declare_list(&name, value);
        self.line(&format!("print(\"{name} {{length({name})}}\")"));
        match (exit, self.returns) {
            ("return", Returns::List) => {
                let returned = self.random.pick(&self.lists).name.clone();
                self.line(&format!("return {returned}"));
            }
            _ => self.line(exit),
        }
        self.lists.pop();
        self.depth -= 1;
    }

    fn condition_on_a_temporary(&mut self) {
        let Some(value) = self.temporary_or_binding() else {
            return;
        };
        let limit = self.random.below(4);
        self.line(&format!("if length({value}) > {limit}"));
        self.line(&format!("    print(\"longer than {limit}\")"));
    }

    /// Prints the total of a list, or of the second half of it, which a slice shows.
    fn show_a_temporary(&mut self) {
        let Some(value) = self.temporary_or_binding() else {
            return;
        };
        if self.random.chance(50) {
            self.line(&format!("print(\"total {{total({value})}}\")"));
            return;
        }
        let half = self.name("half");
        self.line(&format!("constant {half} = {value}"));
        self.line(&format!(
            "print(\"second half {{total(slice({half}, length({half}) / 2, length({half})))}}\")"
        ));
    }
}

#[test]
fn compiled_list_programs_print_what_the_interpreter_prints() {
    let compilers = common::c_compilers();
    let mut random = Random::new(SEED);
    let texts: Vec<String> = (0..PROGRAMS).map(|_| program(&mut random)).collect();
    let tally = compiled::run_all(
        &texts,
        "list_program.lym",
        &compilers,
        |checked, optimized, _| {
            CheckCount::of(checked).list_indexes_checked
                - CheckCount::of(optimized).list_indexes_checked
        },
    );
    assert!(
        tally.agreed * 3 >= PROGRAMS * 2,
        "only {} of {PROGRAMS} list programs ran; {} ran out of steps",
        tally.agreed,
        tally.too_long
    );
    assert!(
        tally.measured > 0,
        "the optimizer removed no list index checks"
    );
    println!(
        "{} of {PROGRAMS} list programs agreed, {} ran out of steps, {} list index checks removed",
        tally.agreed, tally.too_long, tally.measured
    );
}
