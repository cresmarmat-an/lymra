//! The wording of the errors about views, `slice` and `bytes`.

use super::described;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::Type;
use crate::source::Span;

/// `view` without `[element type]`, or with more between the brackets.
pub(in crate::semantic) fn malformed_view_type(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0409,
        "a view type names the type of its elements",
        Label::new(span, "expected `view[element type]`"),
        Help::text(
            "write the type of the elements, as in `view[integer64]`; a view shows any number of them, so it has no length in its type",
        ),
    )
}

pub(in crate::semantic) fn slice_argument_count(found: usize, call: Span) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`slice` takes 3 arguments, but {found} {given} given"),
        Label::new(call, "`slice` takes the elements from a start up to an end"),
        Help::text(
            "write `slice(values, start, end)`, as in `slice(values, 0, 3)` for the first three",
        ),
    )
}

pub(in crate::semantic) fn bytes_argument_count(found: usize, call: Span) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`bytes` takes 1 argument, but {found} {given} given"),
        Label::new(call, "`bytes` shows the bytes of one text"),
        Help::text("write `bytes(text)`"),
    )
}

/// `slice` of a value that has no elements or bytes in order.
pub(in crate::semantic) fn not_sliceable(found: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!(
            "`slice` takes part of an array, a list, a view or a text, but this is {}",
            described(found)
        ),
        Label::new(span, "nothing to take part of"),
        Help::text("pass an array, a list, a view or a `string`"),
    )
}

/// A start or an end of a slice that is not an integer.
pub(in crate::semantic) fn position_not_integer(found: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!(
            "a slice starts and ends at integers, but this is {}",
            described(found)
        ),
        Label::new(span, "not an integer"),
        Help::text("use a whole number from 0 up to the length, the length included"),
    )
}

/**
 * The `part` of a slice, "start" or "end", known while compiling to fall
 * outside: below 0, or past the end of an array of `length` elements.
 */
pub(in crate::semantic) fn slice_position_out_of_range(
    part: &str,
    position: i128,
    length: Option<u64>,
    span: Span,
) -> Diagnostic {
    let (message, help) = match length {
        Some(length) if position >= 0 => (
            format!("slice {part} {position} is out of range for an array of length {length}"),
            format!("a slice of this array starts and ends at 0 to {length}"),
        ),
        _ => (
            format!("slice {part} {position} is negative, and positions count up from 0"),
            "a slice starts and ends at 0 or later, and at most at the length".to_owned(),
        ),
    };
    Diagnostic::new(
        ErrorCode::L0411,
        message,
        Label::new(
            span,
            "the program would stop here every time it ran this line",
        ),
        Help::text(help),
    )
}

/// A slice known while compiling to end before it starts.
pub(in crate::semantic) fn slice_backwards(start: i128, end: i128, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0411,
        format!("slice end {end} is before its start {start}"),
        Label::new(
            span,
            "the program would stop here every time it ran this line",
        ),
        Help::text(
            "a slice ends at its start or after it; an empty slice starts and ends at the same place",
        ),
    )
}

pub(in crate::semantic) fn bytes_of_non_text(found: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!(
            "`bytes` shows the bytes of a text, but this is {}",
            described(found)
        ),
        Label::new(span, "not a `string`"),
        Help::text(
            "pass a `string`; an array, a list or a view goes wherever a view of its elements is expected as it is",
        ),
    )
}

/// An element of a view changed through it, or passed on with `variable`. `source` is the view as written.
pub(in crate::semantic) fn view_element_changed(source: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0208,
        format!("`{source}` is a view, so its elements cannot change through it"),
        Label::new(span, "a view only reads the elements it shows"),
        Help::text(
            "change the element through the array or list the view shows, or pass that one with `variable`",
        ),
    )
}
