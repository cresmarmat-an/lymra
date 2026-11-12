//! The wording of every error the semantic checker reports.

use super::evaluation_order::Conflict;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::{IntegerType, Type};
use crate::source::Span;

/// Help for a misspelled name: a replacement when a close match exists, otherwise `fallback`.
fn spelling_help(span: Span, suggestion: Option<&str>, fallback: &str) -> Help {
    match suggestion {
        Some(name) => Help::replace(format!("did you mean `{name}`?"), span, name),
        None => Help::text(fallback),
    }
}

/// The type with its article, as in "an `integer64`" or "a `boolean`".
pub(super) fn described(ty: Type) -> String {
    let article = match ty.name().as_bytes().first() {
        Some(b'a' | b'e' | b'i' | b'o' | b'u') => "an",
        _ => "a",
    };
    format!("{article} `{ty}`")
}

pub(super) fn unknown_name(name: &str, span: Span, suggestion: Option<&str>) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0206,
        format!("unknown name `{name}`"),
        Label::new(span, "nothing with this name is visible here"),
        spelling_help(
            span,
            suggestion,
            "declare it before this line with `constant` or `variable`, or check the spelling",
        ),
    )
}

pub(super) fn discard_read(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0206,
        "`_` cannot be read",
        Label::new(span, "`_` names a value that is thrown away"),
        Help::text("give the value a real name where it is declared"),
    )
}

/// The error spec section 11 shows: an assignment to a name that has no declaration.
pub(super) fn undeclared_assignment(
    name: &str,
    target: Span,
    suggestion: Option<&str>,
) -> Diagnostic {
    let help = match suggestion {
        Some(similar) => Help::replace(format!("did you mean `{similar}`?"), target, similar),
        None => Help::insert("declare it first", target.start, "variable "),
    };
    Diagnostic::new(
        ErrorCode::L0201,
        format!("assignment to undeclared name `{name}`"),
        Label::new(target, format!("`{name}` is not declared in this scope")),
        help,
    )
}

pub(super) fn discard_assigned(target: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0201,
        "`_` cannot be assigned to",
        Label::new(target, "`_` is not a binding"),
        Help::insert(
            "to compute a value and drop it, declare it as `_`",
            target.start,
            "constant ",
        ),
    )
}

pub(super) fn already_declared(name: &str, span: Span, previous: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0207,
        format!("`{name}` is already declared"),
        Label::new(span, "declared again here"),
        Help::text("choose another name; in Lymra one name never hides another"),
    )
    .with_secondary(Label::new(previous, "first declared here"))
}

pub(super) fn built_in_name(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0207,
        format!("`{name}` is the name of a built-in function"),
        Label::new(span, "this name is taken"),
        Help::text("choose another name"),
    )
}

/// Reports a change to a constant, a parameter or a loop variable. `fix` rewrites the declaration, when one can.
pub(super) fn immutable_changed(
    name: &str,
    noun: &str,
    span: Span,
    declaration: Span,
    fix: Help,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0208,
        format!("`{name}` is a {noun}, so it cannot change"),
        Label::new(span, "changed here"),
        fix,
    )
    .with_secondary(Label::new(
        declaration,
        format!("declared as a {noun} here"),
    ))
}

pub(super) fn never_changed(name: &str, noun: &str, span: Span, fix: Help) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0209,
        format!("the {noun} `{name}` is declared `variable` but never changes"),
        Label::new(span, "nothing changes it"),
        fix,
    )
}

pub(super) fn discard_variable(keyword: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0209,
        "`_` is declared `variable`, but nothing can change it",
        Label::new(keyword, "`_` cannot be named again, so it never changes"),
        Help::replace("declare it with `constant`", keyword, "constant"),
    )
}

pub(super) fn assigned_but_never_read(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0202,
        format!("the variable `{name}` is assigned but never read"),
        Label::new(span, "its value is never used"),
        Help::text("use its value, or remove the variable and its assignments"),
    )
}

pub(super) fn unused(name: &str, noun: &str, span: Span, help: Help) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0202,
        format!("the {noun} `{name}` is never used"),
        Label::new(span, "declared here, never read"),
        help,
    )
}

pub(super) fn not_computable(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0210,
        "a top-level constant has to be known when the program is compiled",
        Label::new(span, "this is only known when the program runs"),
        Help::text(
            "compute it inside a function, or write the value with literals and other top-level constants",
        ),
    )
}

pub(super) fn depends_on_itself(name: &str, span: Span, declaration: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0210,
        format!("the constant `{name}` depends on itself"),
        Label::new(span, "used here while it is being computed"),
        Help::text("compute it from values that do not lead back to it"),
    )
    .with_secondary(Label::new(declaration, "declared here"))
}

pub(super) fn type_mismatch(expected: Type, found: Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!(
            "expected {}, found {}",
            described(expected),
            described(found)
        ),
        Label::new(span, format!("this is {}", described(found))),
        Help::text(
            "Lymra never converts a value to another type by itself; use a value of the expected type",
        ),
    )
}

/// An integer of one type where another integer type is needed, with the conversion as the fix.
pub(super) fn integer_mismatch(
    expected: IntegerType,
    found: Type,
    span: Span,
    source: &str,
) -> Diagnostic {
    let target = expected.name();
    Diagnostic::new(
        ErrorCode::L0402,
        format!(
            "expected an integer of type `{target}`, found {}",
            described(found)
        ),
        Label::new(span, format!("this is {}", described(found))),
        Help::replace(
            format!("convert it with `to_{target}`; the program stops if the value does not fit"),
            span,
            format!("to_{target}({source})"),
        ),
    )
}

/// An integer literal where a value of another type is needed.
pub(super) fn literal_mismatch(expected: Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!("expected {}, found a number", described(expected)),
        Label::new(span, "this is a number"),
        Help::text(format!("write {} here", described(expected))),
    )
}

/// `source` is the condition as written, used to suggest a comparison.
pub(super) fn condition_not_boolean(found: Type, span: Span, source: &str) -> Diagnostic {
    let help = match found {
        Type::Integer(_) => {
            Help::replace("compare it with a number", span, format!("{source} != 0"))
        }
        Type::Boolean | Type::String => Help::text("write a comparison or another `boolean` here"),
    };
    Diagnostic::new(
        ErrorCode::L0402,
        format!(
            "a condition has to be a `boolean`, but this is {}",
            described(found)
        ),
        Label::new(span, "not a `boolean`"),
        help,
    )
}

/// `source` is the argument as written, used to suggest interpolation.
pub(super) fn print_needs_text(found: Type, span: Span, source: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!("`print` takes text, but this is {}", described(found)),
        Label::new(span, "not text"),
        Help::replace(
            "turn the value into text with interpolation",
            span,
            format!("\"{{{source}}}\""),
        ),
    )
}

pub(super) fn no_value(function: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!("`{function}` returns nothing, so this call has no value"),
        Label::new(span, "used as a value here"),
        Help::text(format!(
            "call `{function}` on a line of its own, or give it a return type with `returns`"
        )),
    )
}

pub(super) fn operand_mismatch(
    operator: &str,
    left: (Type, Span),
    right: (Type, Span),
) -> Diagnostic {
    let (left_type, left_span) = left;
    let (right_type, right_span) = right;
    let help = match (left_type, right_type) {
        (Type::Integer(left_integer), Type::Integer(_)) => Help::text(format!(
            "convert one side, for example with `to_{}`; the program stops if the value does not fit",
            left_integer.name()
        )),
        _ => Help::text(
            "Lymra never converts a value to another type by itself; give both sides the same type",
        ),
    };
    Diagnostic::new(
        ErrorCode::L0402,
        format!("the two sides of `{operator}` have different types"),
        Label::new(right_span, format!("this is {}", described(right_type))),
        help,
    )
    .with_secondary(Label::new(
        left_span,
        format!("this is {}", described(left_type)),
    ))
}

pub(super) fn unknown_type(name: &str, span: Span, suggestion: Option<&str>) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0403,
        format!("unknown type `{name}`"),
        Label::new(span, "no type has this name"),
        spelling_help(
            span,
            suggestion,
            "the types are `integer8` to `integer64`, `unsigned8` to `unsigned64`, `boolean` and `string`",
        ),
    )
}

/// `integer` or `unsigned` without the size every Lymra integer type states.
pub(super) fn sizeless_integer(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0403,
        format!("`{name}` is not a type: a Lymra integer type states its size"),
        Label::new(span, "no size given"),
        Help::replace(
            format!("write `{name}64`, or another size such as `{name}32`"),
            span,
            format!("{name}64"),
        ),
    )
}

/// An integer type with a size Lymra does not have, such as `integer128`.
pub(super) fn unsupported_integer_size(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0403,
        format!("unknown type `{name}`"),
        Label::new(span, "no integer type has this size"),
        Help::text(
            "integer types have 8, 16, 32 or 64 bits: `integer8` to `integer64`, `unsigned8` to `unsigned64`",
        ),
    )
}

/// `shown` is how the value appears in the message: the literal itself, or an expression and its value.
pub(super) fn out_of_range(shown: &str, ty: IntegerType, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0404,
        format!("{shown} does not fit in `{}`", ty.name()),
        Label::new(span, format!("outside the range of `{}`", ty.name())),
        Help::text(format!(
            "`{}` holds {} to {}; use a value in that range or a wider type",
            ty.name(),
            ty.minimum(),
            ty.maximum()
        )),
    )
}

pub(super) fn always_overflows(ty: IntegerType, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0404,
        format!("this always overflows `{}`", ty.name()),
        Label::new(span, "the result is known now, and it does not fit"),
        Help::text(format!(
            "`{}` holds {} to {}; the program would stop here every time it ran this line",
            ty.name(),
            ty.minimum(),
            ty.maximum()
        )),
    )
}

pub(super) fn literal_too_large(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0404,
        "this number is too large for any integer type",
        Label::new(span, "too large"),
        Help::text(format!(
            "the largest integer, an `unsigned64`, is {}",
            u64::MAX
        )),
    )
}

pub(super) fn operator_type(message: String, span: Span, label: String, help: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0405,
        message,
        Label::new(span, label),
        Help::text(help),
    )
}

pub(super) fn division_by_zero(divisor: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0406,
        "division by zero",
        Label::new(divisor, "this is always zero"),
        Help::text(
            "divide by a value that is not zero; the program would stop here every time it ran this line",
        ),
    )
}

pub(super) fn argument_count(
    function: &str,
    expected: usize,
    found: usize,
    call: Span,
    declaration: Span,
    signature: &str,
) -> Diagnostic {
    let plural = |count: usize| if count == 1 { "argument" } else { "arguments" };
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!(
            "`{function}` takes {expected} {}, but {found} {given} given",
            plural(expected)
        ),
        Label::new(call, format!("{found} {} here", plural(found))),
        Help::text(format!(
            "pass one argument for each parameter: `{signature}`"
        )),
    )
    .with_secondary(Label::new(
        declaration,
        format!("`{function}` is declared here"),
    ))
}

pub(super) fn conversion_argument_count(function: &str, found: usize, call: Span) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`{function}` takes 1 argument, but {found} {given} given"),
        Label::new(call, "a conversion takes the one value it converts"),
        Help::text(format!("write `{function}(value)`")),
    )
}

pub(super) fn print_argument_count(found: usize, call: Span) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`print` takes 1 argument, but {found} {given} given"),
        Label::new(call, "`print` writes one text and a line break"),
        Help::text("to print several values, put them in one text: `print(\"{first} {second}\")`"),
    )
}

pub(super) fn range_argument_count(found: usize, call: Span) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`range` takes 2 arguments, but {found} {given} given"),
        Label::new(call, "a range needs a start and an end"),
        Help::text("write `range(start, end)`; the end is not included"),
    )
}

pub(super) fn range_arguments_plain(arguments: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0504,
        "the arguments of `range` are plain values",
        Label::new(arguments, "marked or named here"),
        Help::text("pass the start and the end as plain values: `range(0, 10)`"),
    )
}

pub(super) fn missing_return(function: &str, return_type: Type, name: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0502,
        format!(
            "`{function}` can reach its end without returning {}",
            described(return_type)
        ),
        Label::new(
            name,
            format!("declared to return {}", described(return_type)),
        ),
        Help::text("end every path through the function with `return <value>`"),
    )
}

pub(super) fn return_value_without_type(function: &str, value: Span, removal: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0503,
        format!("`{function}` returns nothing, so `return` cannot give a value"),
        Label::new(value, "a value is returned here"),
        Help::remove(
            format!(
                "remove the value, or declare a return type with `returns` after `{function}(...)`"
            ),
            removal,
        ),
    )
}

pub(super) fn return_without_value(function: &str, return_type: Type, keyword: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0503,
        format!(
            "`{function}` returns {}, so `return` needs a value",
            described(return_type)
        ),
        Label::new(keyword, "no value given"),
        Help::text(format!(
            "write `return` followed by {}",
            described(return_type)
        )),
    )
}

pub(super) fn missing_variable_mark(parameter: &str, argument: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0504,
        format!(
            "the parameter `{parameter}` is `variable`, so the argument needs `variable` in front"
        ),
        Label::new(
            argument,
            "the call can change this, and the call has to say so",
        ),
        Help::insert("mark the argument", argument.start, "variable "),
    )
}

pub(super) fn needless_variable_mark(parameter: &str, mark: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0504,
        format!(
            "the parameter `{parameter}` is not `variable`, so the call cannot change this argument"
        ),
        Label::new(mark, "marked as changeable here"),
        Help::remove("remove `variable`", mark),
    )
}

pub(super) fn reference_needs_variable(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0504,
        "only a `variable` binding can be passed with `variable`",
        Label::new(span, "not a binding the call can change"),
        Help::text("declare a `variable`, pass it, and read the result from it afterward"),
    )
}

/// One expression passes a binding with `variable` and also uses it somewhere else.
pub(super) fn changed_and_used(conflict: &Conflict<'_>) -> Diagnostic {
    let name = conflict.name;
    let change = match conflict.function {
        Some(function) => format!("`{function}` may change `{name}` here"),
        None => format!("this call may change `{name}`"),
    };
    Diagnostic::new(
        ErrorCode::L0508,
        format!("`{name}` is passed with `variable` and also used elsewhere in the same expression"),
        Label::new(conflict.changed, change),
        Help::text(format!(
            "make the call a statement of its own and save any result in a `constant`; the statements after it see the new `{name}`"
        )),
    )
    .with_secondary(Label::new(
        conflict.used,
        format!("`{name}` is used here too, so the result depends on which runs first"),
    ))
}

pub(super) fn unused_result(function: &str, statement: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0505,
        format!("the result of `{function}` is not used"),
        Label::new(statement, "the value this call returns is lost"),
        Help::insert(
            "use the value, or discard it on purpose",
            statement.start,
            "constant _ = ",
        ),
    )
}

pub(super) fn unused_value(statement: Span, help: Help) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0505,
        "this value is computed and then thrown away",
        Label::new(statement, "a statement has to do something"),
        help,
    )
}

pub(super) fn main_form(name: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0506,
        "`main` has to take no parameters and return nothing",
        Label::new(name, "the program starts here"),
        Help::text("declare it as `function main()`"),
    )
}

pub(super) fn missing_main(start: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0506,
        "this file has no `main` function, so it cannot run",
        Label::new(start, "a program starts in `function main()`"),
        Help::text("add `function main()` with the code the program runs"),
    )
}

pub(super) fn not_a_function(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0507,
        format!("`{name}` is a value, not a function"),
        Label::new(span, "called here"),
        Help::text("only a function can be called with `(...)`"),
    )
}

pub(super) fn function_as_value(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0507,
        format!("`{name}` is a function, not a value"),
        Label::new(span, "used as a value here"),
        Help::text(format!(
            "call it to use its result: `{name}(...)`; Lymra has no function values"
        )),
    )
}

pub(super) fn not_callable(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0507,
        "only a function name can be called",
        Label::new(span, "this is not a function name"),
        Help::text("call a function by its name, as in `area(2, 3)`"),
    )
}

pub(super) fn function_assigned(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0507,
        format!("`{name}` is a function, so nothing can be assigned to it"),
        Label::new(span, "assigned here"),
        Help::text("assign to a `variable` instead"),
    )
}

pub(super) fn jump_outside_loop(keyword: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0601,
        format!("`{keyword}` outside a loop"),
        Label::new(span, "no `while` or `for` loop encloses this"),
        Help::text(format!(
            "use `{keyword}` inside a loop, or `return` to leave the function"
        )),
    )
}

pub(super) fn unreachable(span: Span, exit: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0602,
        "this code can never run",
        Label::new(span, "unreachable"),
        Help::text("remove it, or move it before the line that leaves"),
    )
    .with_secondary(Label::new(exit, "this always leaves before the code below"))
}
