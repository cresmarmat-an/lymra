//! The wording of the errors about lists.

use super::described;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::Type;
use crate::source::Span;

/// `list` without `[element type]`, or with more between the brackets.
pub(in crate::semantic) fn malformed_list_type(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0409,
        "a list type names the type of its elements",
        Label::new(span, "expected `list[element type]`"),
        Help::text(
            "write the type of the elements, as in `list[integer64]`; a list grows, so it has no length in its type",
        ),
    )
}

/// `list()` where nothing says which list type it builds.
pub(in crate::semantic) fn empty_list_without_type(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0412,
        "`list()` needs to know which list to build",
        Label::new(
            span,
            "its element type comes from the type of the place it goes",
        ),
        Help::text(
            "write the type where the list goes, as in `variable list[integer64] values = list()`",
        ),
    )
}

/// `list()` where a value of a type other than a list goes.
pub(in crate::semantic) fn empty_list_into(expected: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!("expected {}, found a list", described(expected)),
        Label::new(span, "`list()` builds a list"),
        Help::text("use a value of the expected type here; `list()` only goes where a list does"),
    )
}

pub(in crate::semantic) fn empty_list_arguments(found: usize, call: Span) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`list` takes no arguments, but {found} {given} given"),
        Label::new(call, "`list()` builds a list with no elements"),
        Help::text("write `list()`, then add the elements with `push`"),
    )
}

pub(in crate::semantic) fn push_argument_count(found: usize, call: Span) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`push` takes 2 arguments, but {found} {given} given"),
        Label::new(call, "`push` adds one value to the end of one list"),
        Help::text("write `push(variable values, 7)`"),
    )
}

/// The list given to `push` without `variable`, though `push` changes it. `source` is the list as written.
pub(in crate::semantic) fn push_needs_variable(span: Span, source: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0504,
        "`push` changes the list it is given, so the list needs `variable`",
        Label::new(span, "passed without `variable`"),
        Help::replace(
            "mark the list `variable`",
            span,
            format!("variable {source}"),
        ),
    )
}

/// The value given to `push` with `variable`, which `push` only reads.
pub(in crate::semantic) fn pushed_value_marked(mark: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0504,
        "`push` only reads the value it adds, so the value takes no `variable`",
        Label::new(mark, "not needed here"),
        Help::replace("remove `variable`", mark, String::new()),
    )
}

pub(in crate::semantic) fn push_not_list(found: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!("`push` adds to a list, but this is {}", described(found)),
        Label::new(span, "not a list"),
        Help::text("pass a `list` binding, marked `variable`, as in `push(variable values, 7)`"),
    )
}

/// How a list is changed where error L0704 points.
#[derive(Clone, Copy, Debug)]
pub(in crate::semantic) enum Change<'a> {
    Push,
    Replace,
    /// Passed with `variable` to the function named, which can change it.
    Pass(&'a str),
}

/**
 * A `variable` list parameter changed while a list binding of the same
 * function is alive whose copy from an earlier call of the function the
 * parameter can hold. This call's copy sits above that one on their data
 * stack, and only the newest list on a data stack can change. `binding` is
 * the binding's name and where it is declared.
 */
pub(in crate::semantic) fn changed_below_newer(
    parameter: &str,
    change: Change<'_>,
    span: Span,
    binding: (&str, Span),
) -> Diagnostic {
    let (name, declared) = binding;
    let (action, label) = match change {
        Change::Push => ("grow".to_owned(), format!("adds to `{parameter}`")),
        Change::Replace => ("be replaced".to_owned(), format!("replaces `{parameter}`")),
        Change::Pass(callee) => (
            "be passed with `variable`".to_owned(),
            format!("`{callee}` can change `{parameter}`"),
        ),
    };
    Diagnostic::new(
        ErrorCode::L0704,
        format!("`{parameter}` cannot {action} here, where a newer list may sit above it"),
        Label::new(span, label),
        Help::text(format!(
            "only the newest list on a data stack can change; change `{parameter}` before `{name}` is declared, or after the block that declares it ends"
        )),
    )
    .with_secondary(Label::new(
        declared,
        format!("`{parameter}` can be an earlier call's `{name}`, and this `{name}` sits above it"),
    ))
}

/// An index known while compiling to be negative, into a list, whose length only the running program knows.
pub(in crate::semantic) fn negative_index(index: i128, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0411,
        format!("index {index} is negative, and indexes count up from 0"),
        Label::new(
            span,
            "the program would stop here every time it ran this line",
        ),
        Help::text("the first element is at index 0"),
    )
}
