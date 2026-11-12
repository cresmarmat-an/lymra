/*!
 * The optimizer removes overflow and division checks it proves can never
 * fail. A wrong proof would turn a runtime error into a silently wrong
 * number, so these tests hunt for one.
 *
 * They generate thousands of random programs, check them, and run each one
 * in an exact interpreter, first with every check and then optimized. An
 * operation whose check was removed must never overflow or divide by zero,
 * and the optimized run must do exactly what the checked one did. A failure
 * prints the program. The generator has a fixed seed, so it reproduces.
 */

#[path = "../common/mod.rs"]
mod common;
mod generator;
mod grids;
mod interpreter;

use lymra::diagnostics::render_report;
use lymra::frontend;
use lymra::ir::{
    Argument, Block, Call, Expr, ExprKind, FunctionId, IntegerType, PrintPart, Program, Statement,
};
use lymra::optimize;

use generator::Random;

const SEED: u64 = 0x5052_4F4F_4653_0001;
const PROGRAMS: usize = 3_000;
const PROBES: usize = 600;

/// How many integer operations a program has, and how many of them are still checked.
#[derive(Clone, Copy, Debug, Default)]
struct CheckCount {
    operations: usize,
    checked: usize,
}

impl CheckCount {
    fn of(program: &Program<'_>) -> Self {
        let mut count = Self::default();
        for function in &program.functions {
            count.block(&function.body);
        }
        count
    }

    fn block(&mut self, block: &Block) {
        for statement in &block.statements {
            self.statement(statement);
        }
    }

    fn statement(&mut self, statement: &Statement) {
        match statement {
            Statement::Declare { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Discard(value)
            | Statement::Return(Some(value))
            | Statement::Assert { condition: value } => self.expr(value),
            Statement::Call(call) => self.arguments(call),
            Statement::Print(parts) => {
                for part in parts {
                    if let PrintPart::Value(value) = part {
                        self.expr(value);
                    }
                }
            }
            Statement::If {
                branches,
                otherwise,
            } => {
                for branch in branches {
                    self.expr(&branch.condition);
                    self.block(&branch.body);
                }
                if let Some(otherwise) = otherwise {
                    self.block(otherwise);
                }
            }
            Statement::While { condition, body } => {
                self.expr(condition);
                self.block(body);
            }
            Statement::ForRange {
                start, end, body, ..
            } => {
                self.expr(start);
                self.expr(end);
                self.block(body);
            }
            Statement::Return(None) | Statement::Break | Statement::Continue => {}
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Arithmetic {
                left,
                right,
                checked,
                ..
            } => {
                self.operation(*checked);
                self.expr(left);
                self.expr(right);
            }
            ExprKind::Negate { operand, checked } | ExprKind::Convert { operand, checked } => {
                self.operation(*checked);
                self.expr(operand);
            }
            ExprKind::Not(operand) => self.expr(operand),
            ExprKind::Compare { left, right, .. } | ExprKind::Logic { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            ExprKind::Call(call) => self.arguments(call),
            ExprKind::Literal(_) | ExprKind::Local(_) | ExprKind::Constant(_) => {}
        }
    }

    fn arguments(&mut self, call: &Call) {
        for argument in &call.arguments {
            if let Argument::Value(value) = argument {
                self.expr(value);
            }
        }
    }

    fn operation(&mut self, checked: bool) {
        self.operations += 1;
        self.checked += usize::from(checked);
    }
}

/// What the generated programs that passed the checker added up to.
#[derive(Default)]
struct Totals {
    programs: usize,
    operations: usize,
    removed: usize,
}

impl Totals {
    /// Fails when too few programs passed the checker or nothing was removed, since then the test proves little.
    fn assert_meaningful(&self, written: usize) {
        assert!(
            self.programs >= written / 3,
            "only {} of {written} generated programs passed the checker",
            self.programs
        );
        assert!(
            self.removed > 0,
            "the optimizer removed no checks in {} operations",
            self.operations
        );
        println!(
            "{} programs, {} operations, {} checks removed",
            self.programs, self.operations, self.removed
        );
    }
}

fn fail(problem: &str, text: &str) -> ! {
    panic!("{problem}\nin this program:\n{text}")
}

/**
 * Runs `function` on each list of arguments, optimizes the program, and runs
 * it again on each. Fails when a removed check would have failed, or when a
 * run prints or ends differently. Both runs count steps the same way, so even
 * a run that ran out of steps has to match its twin.
 */
fn compare_runs(
    mut program: Program<'_>,
    function: FunctionId,
    inputs: &[Vec<i128>],
    text: &str,
    totals: &mut Totals,
) {
    let checked_runs: Vec<_> = inputs
        .iter()
        .map(|arguments| {
            interpreter::run(&program, function, arguments).unwrap_or_else(|problem| {
                fail(
                    &format!("the checker left an operation unchecked: {problem}"),
                    text,
                )
            })
        })
        .collect();
    let before = CheckCount::of(&program);
    optimize::remove_proven_checks(&mut program);
    let after = CheckCount::of(&program);
    for (arguments, checked_run) in inputs.iter().zip(&checked_runs) {
        let optimized_run =
            interpreter::run(&program, function, arguments).unwrap_or_else(|problem| {
                fail(&format!("{problem}, with arguments {arguments:?}"), text)
            });
        assert_eq!(
            checked_run, &optimized_run,
            "optimizing changed the run with arguments {arguments:?}:\n{text}"
        );
    }
    totals.programs += 1;
    totals.operations += after.operations;
    totals.removed += before.checked - after.checked;
}

/// Compares the checked and optimized runs of a program's `main`. Returns false when the checker rejects the program.
fn check_main(text: &str, totals: &mut Totals) -> bool {
    let source = common::source("generated.lym", text);
    let Ok(program) = frontend::check_program(&source) else {
        return false;
    };
    let main = program.main.expect("a checked program has `main`");
    compare_runs(program, main, &[Vec::new()], text, totals);
    true
}

/// Compares the checked and optimized runs of a program's `probe` on every value of `ty`. Returns false when the checker rejects the program.
fn check_probe(text: &str, ty: IntegerType, totals: &mut Totals) -> bool {
    let source = common::source("probe.lym", text);
    let Ok(program) = frontend::check_program(&source) else {
        return false;
    };
    let probe = program
        .functions
        .iter()
        .position(|function| function.name == generator::PROBE)
        .expect("every probe program has the probe function");
    let inputs: Vec<Vec<i128>> = (ty.minimum()..=ty.maximum())
        .map(|value| vec![value])
        .collect();
    compare_runs(program, probe, &inputs, text, totals);
    true
}

/// Grid programs are written to pass the checker, so a rejected one is a mistake in the grid.
fn assert_accepted(accepted: bool, text: &str) {
    if accepted {
        return;
    }
    let source = common::source("grid.lym", text);
    let report = match frontend::check_program(&source) {
        Err(diagnostics) => render_report(&diagnostics, &source),
        Ok(_) => String::new(),
    };
    panic!("the checker rejected a grid program:\n{text}\n{report}");
}

#[test]
fn removed_checks_never_fail_in_whole_programs() {
    let mut random = Random::new(SEED);
    let mut totals = Totals::default();
    for _ in 0..PROGRAMS {
        // The generator writes some programs the checker rejects, such as ones that always overflow.
        check_main(&generator::program(&mut random), &mut totals);
    }
    totals.assert_meaningful(PROGRAMS);
}

#[test]
fn removed_checks_never_fail_on_any_input() {
    let mut random = Random::new(!SEED);
    let mut totals = Totals::default();
    for _ in 0..PROBES {
        let (text, ty) = generator::probe(&mut random);
        check_probe(&text, ty, &mut totals);
    }
    totals.assert_meaningful(PROBES);
}

#[test]
fn removed_checks_never_fail_at_the_edges_of_conditions() {
    let mut totals = Totals::default();
    let probes = grids::narrowing();
    for probe in &probes {
        assert_accepted(check_probe(&probe.text, probe.ty, &mut totals), &probe.text);
    }
    totals.assert_meaningful(probes.len());
}

#[test]
fn removed_checks_never_fail_at_the_ends_of_loops() {
    let mut totals = Totals::default();
    let programs = grids::accumulation();
    for text in &programs {
        assert_accepted(check_main(text, &mut totals), text);
    }
    let probes = grids::counted_by_input();
    for probe in &probes {
        assert_accepted(check_probe(&probe.text, probe.ty, &mut totals), &probe.text);
    }
    totals.assert_meaningful(programs.len() + probes.len());
}

/// Checks and optimizes a program, then counts its operations and the checks left.
fn optimized_count(name: &str, text: &str) -> CheckCount {
    let source = common::source(name, text);
    let mut program = frontend::check_program(&source)
        .unwrap_or_else(|_| panic!("{name} should pass the checker"));
    optimize::remove_proven_checks(&mut program);
    CheckCount::of(&program)
}

#[test]
fn the_benchmark_loop_loses_every_check() {
    let count = optimized_count(
        "sum.lym",
        "\
function main()
    variable integer64 total = 0
    for round in range(0, 20)
        for index in range(0, 50_000_000)
            total = total + index * 3 + round
    print(\"{total}\")
",
    );
    assert_eq!((count.operations, count.checked), (3, 0));
}

#[test]
fn a_check_that_can_fail_is_kept() {
    let count = optimized_count(
        "level.lym",
        "\
function main()
    variable unsigned8 level = 250
    for _ in range(0, 10)
        level = level + 1
    print(\"{level}\")
",
    );
    assert_eq!((count.operations, count.checked), (1, 1));
}
