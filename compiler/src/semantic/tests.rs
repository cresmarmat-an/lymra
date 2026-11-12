//! Semantic checker tests on the checked program itself. Error messages are covered by `tests/check`.

use crate::diagnostics::ErrorCode;
use crate::frontend;
use crate::ir::{self, IntegerType, Program, Type, Value};
use crate::source::SourceFile;

fn source(text: &str) -> SourceFile {
    SourceFile::new("test.lym", text).expect("test sources are small")
}

/// Checks a program that has to be valid and passes it to `inspect`.
fn with_program(text: &str, inspect: impl FnOnce(&Program<'_>)) {
    let source = source(text);
    match frontend::check(&source) {
        Ok(program) => inspect(&program),
        Err(diagnostics) => panic!("the program should check, but got {diagnostics:#?}"),
    }
}

fn codes(text: &str) -> Vec<ErrorCode> {
    let source = source(text);
    match frontend::check(&source) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect(),
    }
}

fn function<'program, 'src>(
    program: &'program Program<'src>,
    name: &str,
) -> &'program ir::Function<'src> {
    program
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("no function named `{name}`"))
}

fn local<'function, 'src>(
    function: &'function ir::Function<'src>,
    name: &str,
) -> &'function ir::Local<'src> {
    function
        .locals
        .iter()
        .find(|local| local.name == name)
        .unwrap_or_else(|| panic!("no local named `{name}`"))
}

#[test]
fn literals_take_the_type_their_place_asks_for() {
    let text = "\
function main()
    variable unsigned8 small = 200
    small = small + 1
    constant plain = 5
    constant mixed = 1 + small
    print(\"{small} {plain} {mixed}\")
";
    with_program(text, |program| {
        let main = function(program, "main");
        assert_eq!(
            local(main, "small").ty,
            Type::Integer(IntegerType::Unsigned8)
        );
        assert_eq!(
            local(main, "plain").ty,
            Type::Integer(IntegerType::Integer64)
        );
        // A literal on the left takes the type of the typed operand on the right.
        assert_eq!(
            local(main, "mixed").ty,
            Type::Integer(IntegerType::Unsigned8)
        );
    });
}

#[test]
fn top_level_constants_are_computed_while_compiling() {
    let text = "\
constant integer64 minute = 60
constant integer64 hour = minute * 60
constant boolean long = hour > 3_000
constant string unit = \"seconds\"

function main()
    print(\"{hour} {long} {unit}\")
";
    with_program(text, |program| {
        let values: Vec<&Value> = program
            .constants
            .iter()
            .map(|constant| &constant.value)
            .collect();
        assert_eq!(
            values,
            [
                &Value::Integer {
                    value: 60,
                    ty: IntegerType::Integer64
                },
                &Value::Integer {
                    value: 3_600,
                    ty: IntegerType::Integer64
                },
                &Value::Boolean(true),
                &Value::Text("seconds".to_owned()),
            ]
        );
    });
}

#[test]
fn arithmetic_on_known_values_is_folded() {
    let text = "\
constant integer64 base = 40

function main()
    constant answer = base + 2
    print(\"{answer}\")
";
    with_program(text, |program| {
        let main = function(program, "main");
        let Some(ir::Statement::Declare { value, .. }) = main.body.statements.first() else {
            panic!("the first statement declares `answer`");
        };
        assert!(
            matches!(
                value.kind,
                ir::ExprKind::Literal(Value::Integer { value: 42, .. })
            ),
            "{value:?}"
        );
    });
}

#[test]
fn variable_parameters_are_passed_by_reference() {
    let text = "\
function increment(variable integer64 count, integer64 step)
    count = count + step

function main()
    variable integer64 total = 0
    increment(variable total, 2)
    print(\"{total}\")
";
    with_program(text, |program| {
        let increment = function(program, "increment");
        assert!(local(increment, "count").by_reference);
        assert!(local(increment, "count").mutable);
        assert!(!local(increment, "step").by_reference);
        assert!(!local(increment, "step").mutable);

        let main = function(program, "main");
        let Some(ir::Statement::Call(call)) = main.body.statements.get(1) else {
            panic!("the second statement calls `increment`");
        };
        assert!(matches!(
            call.arguments.first(),
            Some(ir::Argument::Reference(_))
        ));
        assert!(matches!(
            call.arguments.get(1),
            Some(ir::Argument::Value(_))
        ));
    });
}

#[test]
fn a_loop_variable_has_the_type_of_its_range() {
    let text = "\
function main()
    constant unsigned16 last = 3
    for index in range(0, last)
        print(\"{index}\")
";
    with_program(text, |program| {
        let main = function(program, "main");
        assert_eq!(
            local(main, "index").ty,
            Type::Integer(IntegerType::Unsigned16)
        );
        assert!(!local(main, "index").mutable);
    });
}

#[test]
fn the_entry_point_is_found_when_present() {
    with_program("function main()\n    return\n", |program| {
        assert_eq!(program.main, Some(0));
    });
    with_program("public constant integer64 answer = 42\n", |program| {
        assert_eq!(program.main, None);
    });
    let source = source("public constant integer64 answer = 42\n");
    let Err(diagnostics) = frontend::check_program(&source) else {
        panic!("a file without `main` is not a program");
    };
    assert_eq!(
        diagnostics.first().map(|diagnostic| diagnostic.code),
        Some(ErrorCode::L0506)
    );
}

#[test]
fn errors_come_in_source_order_even_across_functions() {
    let text = "\
function first()
    unknown_one()

constant computed = first()

function main()
    unknown_two()
";
    assert_eq!(
        codes(text),
        [ErrorCode::L0206, ErrorCode::L0402, ErrorCode::L0206]
    );
}

#[test]
fn syntax_errors_stop_before_meaning_is_checked() {
    // `string b = a` is a syntax error; the unknown name `a` would only be an echo of it.
    assert_eq!(
        codes("function main()\n    string b = a\n"),
        [ErrorCode::L0203]
    );
}

#[test]
fn a_while_true_loop_without_break_never_falls_through() {
    let forever = "\
function wait() returns integer64
    while true
        return 1

function main()
    print(\"{wait()}\")
";
    assert_eq!(codes(forever), []);
    let breaks = "\
function wait() returns integer64
    while true
        break
    print(\"after\")

function main()
    print(\"{wait()}\")
";
    assert_eq!(codes(breaks), [ErrorCode::L0502]);
}
