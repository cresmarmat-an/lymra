//! The wording of the errors about functions that can fail: `fails`, `fail` and `try` (L0801 to L0805).

use super::described;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::Type;
use crate::source::Span;

/// `fail` in a function that cannot fail, with a value of type `value`.
pub(in crate::semantic) fn fail_without_fails(
    function: &str,
    value: &Type,
    keyword: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0801,
        format!("`fail` ends a function that can fail, but `{function}` cannot fail"),
        Label::new(
            keyword,
            format!("this would end `{function}` with {}", described(value)),
        ),
        Help::text(format!(
            "add `fails {value}` to the declaration of `{function}`, after its parameters and its return type"
        )),
    )
}

/// `try` that passes on a failure of type `passed` in a function that cannot fail.
pub(in crate::semantic) fn try_without_fails(
    function: &str,
    passed: &Type,
    keyword: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0801,
        format!("`try` passes a failure on, but `{function}` cannot fail"),
        Label::new(
            keyword,
            format!(
                "on a failure this would end `{function}` with {}",
                described(passed)
            ),
        ),
        Help::text(format!(
            "add `fails {passed}` to the declaration of `{function}`, or handle both cases here with `match`"
        )),
    )
}

/// `try` that passes on a failure of type `passed` in a function that fails with `declared`.
pub(in crate::semantic) fn try_wrong_failure(
    function: &str,
    passed: &Type,
    declared: &Type,
    keyword: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0801,
        format!(
            "`try` would pass on {}, but `{function}` fails with {}",
            described(passed),
            described(declared)
        ),
        Label::new(keyword, "a failure never changes type by itself"),
        Help::text(format!(
            "fail with {} of your own: `try call else fail value`, or handle both cases with `match`",
            described(declared)
        )),
    )
}

/// `fails` with a type that cannot be a failure.
pub(in crate::semantic) fn failure_type_not_allowed(ty: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0802,
        format!("a function cannot fail with {}", described(ty)),
        Label::new(
            span,
            "a failure is a value that does not grow and holds no view or handle",
        ),
        Help::text(
            "fail with a choice or a declared type made of numbers, booleans and text, as in `fails ParseError`",
        ),
    )
}

/// `try` before something that is not a call to a function that can fail; `what` says what it is.
pub(in crate::semantic) fn try_needs_failing_call(what: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0803,
        "`try` needs a call to a function that can fail",
        Label::new(span, what),
        Help::text(
            "remove `try`, or put it right before a call to a function declared with `fails`",
        ),
    )
}

/// `main` declared to fail with something other than text.
pub(in crate::semantic) fn main_fails_with(ty: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0804,
        "`main` can only fail with text",
        Label::new(
            span,
            format!("{} can't be written after `error: `", described(ty)),
        ),
        Help::replace(
            "declare `main` with `fails string`, and turn other failures into text with `try call else fail \"what went wrong\"`",
            span,
            "string",
        ),
    )
}

/// `try call else fail value` inside a larger expression.
pub(in crate::semantic) fn fallback_inside_expression(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0805,
        "`try` with `else fail` has to be the whole value of its statement",
        Label::new(span, "this is part of a larger expression"),
        Help::text(
            "give it a statement of its own, as in `constant value = try read(path) else fail \"cannot read\"`, and use `value` here",
        ),
    )
}

/**
 * A call to a function that can fail, as a statement of its own, whose
 * result nothing handles. A value the function returns has to be kept or
 * discarded on purpose too.
 */
pub(in crate::semantic) fn unhandled_failure(
    function: &str,
    returns_value: bool,
    statement: Span,
) -> Diagnostic {
    let (message, insertion) = if returns_value {
        (
            "pass the failure on with `try` and discard the value on purpose, or handle both cases with `match`",
            "constant _ = try ",
        )
    } else {
        (
            "pass the failure on with `try`, or handle both cases with `match`",
            "try ",
        )
    };
    Diagnostic::new(
        ErrorCode::L0505,
        format!("`{function}` can fail, and nothing handles its result"),
        Label::new(statement, "a failure here would go unnoticed"),
        Help::insert(message, statement.start, insertion),
    )
}
