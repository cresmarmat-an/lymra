//! Parser tests. Trees are compared in the outline form that `ast::print_module` produces.

use super::MAX_OPERATIONS;
use crate::ast::print_module;
use crate::diagnostics::ErrorCode;
use crate::frontend::analyze;
use crate::source::SourceFile;

fn source(text: &str) -> SourceFile {
    SourceFile::new("test.lym", text).expect("test sources are small")
}

/// The tree of a source that must parse without errors.
fn tree(text: &str) -> String {
    let source = source(text);
    let analysis = analyze(&source);
    assert!(
        analysis.diagnostics.is_empty(),
        "unexpected errors for {text:?}: {:#?}",
        analysis.diagnostics
    );
    print_module(&analysis.module)
}

/// The tree of a function body that must parse without errors, without the wrapper lines.
fn body(statements: &str) -> String {
    let indented: String = statements
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect();
    let printed = tree(&format!("function test()\n{indented}"));
    let lines = printed
        .lines()
        .skip(2)
        .map(|line| line.strip_prefix("    ").unwrap_or(line));
    lines.collect::<Vec<_>>().join("\n")
}

/// The prefix form of one expression.
fn expr(text: &str) -> String {
    body(&format!("x = {text}"))
        .strip_prefix("assign x = ")
        .expect("an assignment")
        .to_owned()
}

fn codes(text: &str) -> Vec<ErrorCode> {
    let source = source(text);
    analyze(&source)
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

/// Error codes for statements placed inside a function body.
fn body_codes(statements: &str) -> Vec<ErrorCode> {
    let indented: String = statements
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect();
    codes(&format!("function test()\n{indented}"))
}

#[test]
fn multiplication_binds_tighter_than_addition() {
    assert_eq!(expr("1 + 2 * 3"), "(+ 1 (* 2 3))");
    assert_eq!(expr("1 * 2 + 3"), "(+ (* 1 2) 3)");
    assert_eq!(expr("(1 + 2) * 3"), "(* (+ 1 2) 3)");
}

#[test]
fn operators_of_equal_precedence_group_to_the_left() {
    assert_eq!(expr("a - b - c"), "(- (- a b) c)");
    assert_eq!(expr("a / b * c % d"), "(% (* (/ a b) c) d)");
}

#[test]
fn logic_binds_looser_than_comparison() {
    assert_eq!(
        expr("a < b and c >= d or e"),
        "(or (and (< a b) (>= c d)) e)"
    );
    assert_eq!(expr("a or b and c"), "(or a (and b c))");
}

#[test]
fn not_covers_a_whole_comparison_but_not_an_and() {
    assert_eq!(expr("not a == b"), "(not (== a b))");
    assert_eq!(expr("not a and b"), "(and (not a) b)");
    assert_eq!(expr("not not a"), "(not (not a))");
}

#[test]
fn unary_minus_binds_tighter_than_any_binary_operator() {
    assert_eq!(expr("-a * b"), "(* (- a) b)");
    assert_eq!(expr("a - -b"), "(- a (- b))");
    assert_eq!(expr("-a.b"), "(- (. a b))");
}

#[test]
fn postfix_forms_chain() {
    assert_eq!(expr("a.b.c"), "(. (. a b) c)");
    assert_eq!(expr("grid[i][j]"), "(index (index grid i) j)");
    assert_eq!(
        expr("geometry.area(2.0, 3.0)"),
        "(call (. geometry area) 2.0 3.0)"
    );
    assert_eq!(expr("make()()"), "(call (call make))");
}

#[test]
fn arguments_can_be_positional_named_or_mutable() {
    assert_eq!(
        expr("Frog(name = \"Kero\", age = 3)"),
        "(call Frog name=(text \"Kero\") age=3)"
    );
    assert_eq!(
        expr("push(variable items, 7)"),
        "(call push (variable items) 7)"
    );
    assert_eq!(
        expr("set(variable grid[0].cell, 1)"),
        "(call set (variable (. (index grid 0) cell)) 1)"
    );
}

#[test]
fn text_keeps_its_parts_in_order() {
    assert_eq!(expr("\"\""), "(text)");
    assert_eq!(expr("\"Hello, {name}!\""), "(text \"Hello, \" name \"!\")");
    assert_eq!(expr("\"{a + 1}{b}\""), "(text (+ a 1) b)");
    assert_eq!(expr("\"{{x}} \\n\""), "(text \"{x} \\n\")");
    assert_eq!(
        expr("\"{greet(\"you\")}\""),
        "(text (call greet (text \"you\")))"
    );
}

#[test]
fn literals_keep_their_values() {
    assert_eq!(expr("0xFF + 1_000"), "(+ 255 1000)");
    assert_eq!(expr("1.0e-9"), "1e-9");
    assert_eq!(expr("true or false"), "(or true false)");
}

#[test]
fn bindings_have_an_optional_type() {
    assert_eq!(
        body("variable string name = \"Cresmar\""),
        "variable string name = (text \"Cresmar\")"
    );
    assert_eq!(body("variable count = 0"), "variable count = 0");
    assert_eq!(
        body("constant list[integer32] numbers = list()"),
        "constant list[integer32] numbers = (call list)"
    );
    assert_eq!(
        body("constant geometry.Shape shape = load()"),
        "constant geometry.Shape shape = (call load)"
    );
}

#[test]
fn types_nest_and_take_sizes() {
    let printed =
        tree("type Grid\n    array[array[float32, 4], 4] cells\n    list[packed Shape] shapes\n");
    assert!(
        printed.contains("field array[array[float32, 4], 4] cells"),
        "{printed}"
    );
    assert!(
        printed.contains("field list[packed Shape] shapes"),
        "{printed}"
    );
}

#[test]
fn an_if_chain_is_one_statement() {
    let printed = body("if a\n    one()\nelse if b\n    two()\nelse\n    three()");
    let expected = "\
if a
  expression (call one)
else if b
  expression (call two)
else
  expression (call three)";
    assert_eq!(printed, expected);
}

#[test]
fn loops_and_jumps() {
    let printed = body(
        "while count < 10\n    count = count + 1\nfor item in items\n    if item\n        continue\n    break",
    );
    let expected = "\
while (< count 10)
  assign count = (+ count 1)
for item in items
  if item
    continue
  break";
    assert_eq!(printed, expected);
}

#[test]
fn a_match_lists_its_cases() {
    let printed = body(
        "match shape\n    case Circle(radius)\n        return radius\n    case Dot\n        return 0.0",
    );
    let expected = "\
match shape
  case Circle(radius)
    return radius
  case Dot
    return 0.0";
    assert_eq!(printed, expected);
}

#[test]
fn a_with_block_belongs_to_the_call_before_it() {
    let printed = body(
        "constant evens = filter(numbers) with n\n    n % 2 == 0\nsort(variable items) with a, b\n    a < b",
    );
    let expected = "\
constant evens = (call filter numbers) with n
  expression (== (% n 2) 0)
expression (call sort (variable items)) with a, b
  expression (< a b)";
    assert_eq!(printed, expected);
}

#[test]
fn a_statement_continues_across_lines_inside_brackets() {
    assert_eq!(
        body("total = add(\n    1,\n        2\n)"),
        "assign total = (call add 1 2)"
    );
}

#[test]
fn declarations_at_the_top_level() {
    let printed = tree(
        "#!edition 0.1\n#!optimize speed\nuse math.geometry\n\npublic constant integer64 limit = 10\n\n\
         public function area(float64 width, variable list[integer32] seen) returns float64\n    return width\n\n\
         choice Shape\n    Circle(float64 radius)\n    Dot\n",
    );
    let expected = "\
module
  directive edition 0.1
  directive optimize speed
  use math.geometry
  public constant integer64 limit = 10
  public function area(float64 width, variable list[integer32] seen) returns float64
    return width
  choice Shape
    variant Circle(float64 radius)
    variant Dot
";
    assert_eq!(printed, expected);
}

#[test]
fn each_syntax_mistake_is_one_error() {
    let cases: &[(&str, ErrorCode)] = &[
        ("print(\"outside\")\n", ErrorCode::L0302),
        ("variable count = 0\n", ErrorCode::L0205),
        (
            "function main()\n    print(1)\n#!optimize speed\n",
            ErrorCode::L0101,
        ),
        ("#!optimise speed\n", ErrorCode::L0105),
        ("#!nocheck\n", ErrorCode::L0105),
        ("#!edition 9.9\n", ErrorCode::L0106),
        ("#!optimize speed\n#!optimize size\n", ErrorCode::L0106),
        ("function main()\nprint(1)\n", ErrorCode::L0303),
        ("function main()\n    a()\n        b()\n", ErrorCode::L0304),
        ("fn main()\n    a()\n", ErrorCode::L0305),
        ("function f(i64 a)\n    a()\n", ErrorCode::L0305),
        (
            "extern function hypot(float64 x) returns float64\n",
            ErrorCode::L0309,
        ),
        (
            "function main() -> integer64\n    return 1\n",
            ErrorCode::L0305,
        ),
        ("function main():\n    a()\n", ErrorCode::L0305),
        ("function f(x: integer64)\n    a()\n", ErrorCode::L0305),
        ("use geometry from math\n", ErrorCode::L0305),
        (
            "function first[T](view[T] xs) returns T\n    return xs[0]\n",
            ErrorCode::L0309,
        ),
        (
            "external function hypot(float64 x, float64 y) returns float64\n",
            ErrorCode::L0309,
        ),
        ("fucntion main()\n    a()\n", ErrorCode::L0301),
        ("function f(x)\n    a()\n", ErrorCode::L0301),
        ("function f(integer64 a,)\n    a()\n", ErrorCode::L0301),
        ("public use math\n", ErrorCode::L0301),
    ];
    for (source, expected) in cases {
        assert_eq!(codes(source), [*expected], "for source {source:?}");
    }
}

#[test]
fn each_statement_mistake_is_one_error() {
    let cases: &[(&str, ErrorCode)] = &[
        ("string b = a", ErrorCode::L0203),
        ("list[integer32] xs = ys", ErrorCode::L0203),
        ("variable integer64 total", ErrorCode::L0204),
        ("variable total", ErrorCode::L0204),
        ("let total = 0", ErrorCode::L0305),
        ("variable total: integer64 = 0", ErrorCode::L0305),
        ("count += 1", ErrorCode::L0305),
        ("count++", ErrorCode::L0305),
        ("count = 1;", ErrorCode::L0305),
        ("for i in 0..10\n    a()", ErrorCode::L0305),
        ("if a\n    b()\nelif c\n    d()", ErrorCode::L0305),
        ("if a:\n    b()", ErrorCode::L0305),
        ("if a then\n    b()", ErrorCode::L0305),
        ("x = a < b < c", ErrorCode::L0306),
        ("x = 1 + f(y) with n\n    n", ErrorCode::L0307),
        ("f() = 1", ErrorCode::L0308),
        ("f(variable 5)", ErrorCode::L0308),
        ("if a = b\n    c()", ErrorCode::L0301),
        ("x = (1 + 2", ErrorCode::L0301),
        ("x = f(1, 2", ErrorCode::L0301),
        ("x = items[0", ErrorCode::L0301),
        ("x = ()", ErrorCode::L0301),
        ("x = 1 +", ErrorCode::L0301),
        ("a() b()", ErrorCode::L0301),
        ("whlie x < 3\n    a()", ErrorCode::L0301),
        ("else\n    a()", ErrorCode::L0301),
        ("variable type = 1", ErrorCode::L0301),
        ("x = \"{}\"", ErrorCode::L0113),
        ("function inner()\n    a()", ErrorCode::L0302),
    ];
    for (statements, expected) in cases {
        assert_eq!(
            body_codes(statements),
            [*expected],
            "for statements {statements:?}"
        );
    }
}

#[test]
fn a_lexer_error_is_not_repeated_by_the_parser() {
    assert_eq!(body_codes("x = $"), [ErrorCode::L0108]);
    assert_eq!(body_codes("print(\"open)"), [ErrorCode::L0111]);
    assert_eq!(body_codes("x = 12abc + 1"), [ErrorCode::L0114]);
}

#[test]
fn parsing_continues_after_an_error() {
    let source = "function broken(\n    return 1\n\nfunction fine() returns integer64\n    x = (\n    return 2\n";
    assert_eq!(codes(source).len(), 2);
    let source = self::source(source);
    let printed = print_module(&analyze(&source).module);
    assert!(
        printed.contains("function fine() returns integer64\n    return 2\n"),
        "{printed}"
    );
}

#[test]
fn a_rejected_if_takes_its_else_with_it() {
    assert_eq!(
        body_codes("if a +\n    b()\nelse if c\n    d()\nelse\n    e()\nf()"),
        [ErrorCode::L0301]
    );
}

#[test]
fn foreign_spellings_are_read_as_lymra() {
    let source = source(
        "fn main()\n    let total = 1\n    if total == 1:\n        print(total)\n    elif total != 2 then\n        total += 1\n",
    );
    let analysis = analyze(&source);
    let reported: Vec<ErrorCode> = analysis
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert_eq!(reported, [ErrorCode::L0305; 6]);
    let printed = print_module(&analysis.module);
    assert!(
        printed.contains("function main()\n    constant total = 1\n    if (== total 1)\n"),
        "{printed}"
    );
    assert!(printed.contains("    else if (!= total 2)\n"), "{printed}");
}

#[test]
fn nesting_past_the_limit_is_an_error_not_a_crash() {
    let deep = format!("{}1{}", "(".repeat(5_000), ")".repeat(5_000));
    assert_eq!(body_codes(&format!("x = {deep}")), [ErrorCode::L0310]);
    let negations = format!("x = {}1", "- ".repeat(5_000));
    assert_eq!(body_codes(&negations), [ErrorCode::L0310]);
    let shallow = format!("{}1{}", "(".repeat(40), ")".repeat(40));
    assert_eq!(expr(&shallow), "1");
}

#[test]
fn an_expression_past_the_operation_limit_is_an_error_not_a_crash() {
    std::thread::Builder::new()
        .stack_size(crate::STACK_SIZE)
        .spawn(check_operation_limit)
        .expect("the system starts a thread with the compiler's stack")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn check_operation_limit() {
    let limit = usize::try_from(MAX_OPERATIONS).expect("the limit fits a usize");
    let sum = |operators: usize| vec!["1"; operators + 1].join(" + ");
    assert_eq!(body_codes(&format!("x = {}", sum(limit))), []);
    assert_eq!(
        body_codes(&format!("x = {}", sum(limit + 1))),
        [ErrorCode::L0310]
    );
    let calls = format!("x = f{}", "()".repeat(limit + 1));
    assert_eq!(body_codes(&calls), [ErrorCode::L0310]);
    let indexes = format!("x = items{}", "[0]".repeat(limit + 1));
    assert_eq!(body_codes(&indexes), [ErrorCode::L0310]);
    let fields = format!("x = item{}", ".part".repeat(limit + 1));
    assert_eq!(body_codes(&fields), [ErrorCode::L0310]);
    // Operations inside parentheses and arguments count toward the expression around them.
    let split = format!("x = f({}) + ({})", sum(limit / 2), sum(limit / 2));
    assert_eq!(body_codes(&split), [ErrorCode::L0310]);
    // Each statement starts a new count.
    let two = format!("x = {}\ny = {}", sum(limit), sum(limit));
    assert_eq!(body_codes(&two), []);
}
