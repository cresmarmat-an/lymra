//! The wording of the errors about what views show: L0701 and L0703.

use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::source::Span;

/// How a list is replaced where error L0703 points.
#[derive(Clone, Copy, Debug)]
pub(in crate::semantic) enum Replacement<'a> {
    /// Assigned a whole new value; the span is the target as written.
    Assigned(Span),
    /// Passed with `variable` to the function named, which can replace it; the span is the argument as written.
    Passed(Span, &'a str),
}

/// A list replaced while a `for` loop goes over it, or over a view of it, named at `iterable`.
pub(in crate::semantic) fn replaced_while_iterated(
    list: &str,
    how: Replacement<'_>,
    iterable: Span,
) -> Diagnostic {
    let (message, span, label, help) = match how {
        Replacement::Assigned(span) => (
            format!("`{list}` cannot be replaced while a `for` loop goes over it"),
            span,
            "this replaces the whole list".to_owned(),
            "change its elements or `push` to it inside the loop, and replace it after the loop ends"
                .to_owned(),
        ),
        Replacement::Passed(span, function) => (
            format!("`{list}` cannot be passed with `variable` while a `for` loop goes over it"),
            span,
            format!("`{function}` can replace the whole list"),
            format!(
                "call `{function}` after the loop; `push` and changing elements are fine inside it"
            ),
        ),
    };
    Diagnostic::new(
        ErrorCode::L0703,
        message,
        Label::new(span, label),
        Help::text(help),
    )
    .with_secondary(Label::new(iterable, "the loop goes over it here"))
}

/// A list replaced while `view`, declared at `declared`, still shows it.
pub(in crate::semantic) fn replaced_while_viewed(
    list: &str,
    view: &str,
    how: Replacement<'_>,
    declared: Span,
) -> Diagnostic {
    let (message, span, label) = match how {
        Replacement::Assigned(span) => (
            format!("`{list}` cannot be replaced while `{view}`, a view of it, is still used"),
            span,
            format!("this frees the elements `{view}` shows"),
        ),
        Replacement::Passed(span, function) => (
            format!(
                "`{list}` cannot be passed with `variable` while `{view}`, a view of it, is still used"
            ),
            span,
            format!("`{function}` can replace the whole list"),
        ),
    };
    Diagnostic::new(
        ErrorCode::L0703,
        message,
        Label::new(span, label),
        Help::text(format!(
            "finish with `{view}` before `{list}` changes, or copy what `{view}` shows into a list of its own"
        )),
    )
    .with_secondary(Label::new(declared, format!("`{view}` shows `{list}`")))
}

/// A view binding given a view of a value its statement frees.
pub(in crate::semantic) fn view_of_temporary(view: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{view}` would show a value that is freed at the end of this statement"),
        Label::new(
            span,
            "this value has no binding, so it lasts only as long as its statement",
        ),
        Help::text(format!(
            "keep the value in a binding first, then give `{view}` a view of that binding"
        )),
    )
}

/// A view binding given a view of `root`, declared at `declared` in a block that ends before the view's.
pub(in crate::semantic) fn view_outlives_root(
    view: &str,
    root: &str,
    span: Span,
    declared: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{view}` would outlive `{root}`, which it shows"),
        Label::new(span, format!("`{view}` shows `{root}` from here")),
        Help::text(format!(
            "use `{view}` only inside the block of `{root}`, or keep what it shows in a binding declared where `{view}` is"
        )),
    )
    .with_secondary(Label::new(
        declared,
        format!("`{root}` is freed at the end of its block, before `{view}` is"),
    ))
}

/// A `variable` view parameter given a view of `root`, a binding of `function`, which the caller would keep.
pub(in crate::semantic) fn view_escapes_call(
    view: &str,
    root: &str,
    function: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{view}` would show `{root}`, which is freed when `{function}` returns"),
        Label::new(span, format!("the caller keeps `{view}` after the call")),
        Help::text(format!(
            "give `{view}` a part of what it showed, or a view of another parameter"
        )),
    )
}

/// A function that returns a view of a value its `return` frees.
pub(in crate::semantic) fn returned_view_of_temporary(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        "this returns a view of a value that is freed at the end of the `return`",
        Label::new(
            span,
            "this value has no binding, so it lasts only as long as its statement",
        ),
        Help::text("return the value itself, as a list or an array, instead of a view of it"),
    )
}

/// A function that returns a view of `root`, one of its own bindings.
pub(in crate::semantic) fn returned_view_of_local(
    root: &str,
    function: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{function}` returns a view of `{root}`, which is freed when it returns"),
        Label::new(span, format!("a view of `{root}`")),
        Help::text(format!(
            "return `{root}` itself, or a view of one of the parameters, which the caller keeps"
        )),
    )
}
