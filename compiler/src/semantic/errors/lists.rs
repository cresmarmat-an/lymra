//! The wording of the errors about lists and stacks.

use super::described;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::{Sequence, Type};
use crate::source::Span;

/// `list` or `stack` without `[element type]`, or with more between the brackets.
pub(in crate::semantic) fn malformed_list_type(kind: Sequence, span: Span) -> Diagnostic {
    let name = kind.name();
    Diagnostic::new(
        ErrorCode::L0409,
        format!("a {name} type names the type of its elements"),
        Label::new(span, format!("expected `{name}[element type]`")),
        Help::text(format!(
            "write the type of the elements, as in `{name}[integer64]`; a {name} grows, so it has no length in its type"
        )),
    )
}

/// `list()` or `stack()` where nothing says which type it builds.
pub(in crate::semantic) fn empty_list_without_type(kind: Sequence, span: Span) -> Diagnostic {
    let name = kind.name();
    Diagnostic::new(
        ErrorCode::L0412,
        format!("`{name}()` needs to know which {name} to build"),
        Label::new(
            span,
            "its element type comes from the type of the place it goes",
        ),
        Help::text(format!(
            "write the type where the {name} goes, as in `variable {name}[integer64] values = {name}()`"
        )),
    )
}

/// `list()` or `stack()` where a value of another type goes.
pub(in crate::semantic) fn empty_list_into(
    kind: Sequence,
    expected: &Type,
    span: Span,
) -> Diagnostic {
    let name = kind.name();
    Diagnostic::new(
        ErrorCode::L0402,
        format!("expected {}, found a {name}", described(expected)),
        Label::new(span, format!("`{name}()` builds a {name}")),
        Help::text(format!(
            "use a value of the expected type here; `{name}()` only goes where a {name} does"
        )),
    )
}

pub(in crate::semantic) fn empty_list_arguments(
    kind: Sequence,
    found: usize,
    call: Span,
) -> Diagnostic {
    let name = kind.name();
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`{name}` takes no arguments, but {found} {given} given"),
        Label::new(call, format!("`{name}()` builds a {name} with no elements")),
        Help::text(format!(
            "write `{name}()`, then add the elements with `push`"
        )),
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

pub(in crate::semantic) fn stack_argument_count(
    function: &str,
    found: usize,
    call: Span,
) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    let (label, help) = if function == "pop" {
        (
            "`pop` takes the top element off one stack",
            "write `pop(variable values)`",
        )
    } else {
        (
            "`top` reads the top element of one stack",
            "write `top(values)`",
        )
    };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`{function}` takes 1 argument, but {found} {given} given"),
        Label::new(call, label),
        Help::text(help),
    )
}

/// The stack given to `pop` without `variable`, though `pop` changes it. `source` is the stack as written.
pub(in crate::semantic) fn pop_needs_variable(span: Span, source: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0504,
        "`pop` changes the stack it is given, so the stack needs `variable`",
        Label::new(span, "passed without `variable`"),
        Help::replace(
            "mark the stack `variable`",
            span,
            format!("variable {source}"),
        ),
    )
}

/// `pop` or `top`, named by `function`, of a value that is not a stack.
pub(in crate::semantic) fn not_a_stack(function: &str, found: &Type, span: Span) -> Diagnostic {
    let help = match found {
        Type::List(_) => {
            "a list only grows; declare it as a `stack` to take elements off, or read the last one as `values[length(values) - 1]`"
        }
        _ => "pass a `stack`, as in `variable stack[integer64] values = stack()`",
    };
    Diagnostic::new(
        ErrorCode::L0402,
        format!(
            "`{function}` works on a stack, but this is {}",
            described(found)
        ),
        Label::new(span, "not a stack"),
        Help::text(help),
    )
}

/// How a list is changed where error L0704 points.
#[derive(Clone, Copy, Debug)]
pub(in crate::semantic) enum Change<'a> {
    Push,
    Pop,
    Replace,
    /// Passed with `variable` to the function named, which can change it.
    Pass(&'a str),
}

/**
 * A `variable` list or stack parameter changed while a binding of the same
 * function is alive whose copy from an earlier call of the function the
 * parameter can hold. This call's copy sits above that one on their data
 * stack, and only the newest value on a data stack can change. `binding` is
 * the binding's name and where it is declared; `kind` is what both are.
 */
pub(in crate::semantic) fn changed_below_newer(
    parameter: &str,
    change: Change<'_>,
    span: Span,
    binding: (&str, Span),
    kind: Sequence,
) -> Diagnostic {
    let (name, declared) = binding;
    let noun = kind.name();
    let (action, label) = match change {
        Change::Push => ("grow".to_owned(), format!("adds to `{parameter}`")),
        Change::Pop => (
            "shrink".to_owned(),
            format!("takes an element off `{parameter}`"),
        ),
        Change::Replace => ("be replaced".to_owned(), format!("replaces `{parameter}`")),
        Change::Pass(callee) => (
            "be passed with `variable`".to_owned(),
            format!("`{callee}` can change `{parameter}`"),
        ),
    };
    Diagnostic::new(
        ErrorCode::L0704,
        format!("`{parameter}` cannot {action} here, where a newer {noun} may sit above it"),
        Label::new(span, label),
        Help::text(format!(
            "only the newest {noun} on a data stack can change; change `{parameter}` before `{name}` is declared, or after the block that declares it ends"
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
