//! The wording of the errors about arrays.

use super::{described, operator_type};
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::Type;
use crate::semantic::arrays::MAX_ARRAY_SIZE;
use crate::source::Span;

/// `array` without `[element type, length]`, or with something else between the brackets.
pub(in crate::semantic) fn malformed_array_type(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0409,
        "an array type names its element type and its length",
        Label::new(span, "expected `array[element type, length]`"),
        Help::text(
            "write the type of the elements, then how many there are, as in `array[integer64, 10]`",
        ),
    )
}

pub(in crate::semantic) fn empty_array(length: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0409,
        "an array holds at least one element",
        Label::new(length, "a length of 0"),
        Help::text("give the number of elements the array holds, 1 or more"),
    )
}

/// A length given by a top-level constant whose value is negative.
pub(in crate::semantic) fn negative_length(name: &str, value: i128, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0409,
        format!("an array holds at least one element, but `{name}` is {value}"),
        Label::new(span, "a negative length"),
        Help::text("give the number of elements the array holds, 1 or more"),
    )
}

/// A type such as `integer64[3]`, written with brackets that only `array` takes.
pub(in crate::semantic) fn sized_scalar(name: &str, length: Option<u64>, span: Span) -> Diagnostic {
    let help = match length {
        Some(length) => Help::replace(
            "an array of them is written with `array`",
            span,
            format!("array[{name}, {length}]"),
        ),
        None => Help::text("an array is written `array[element type, length]`"),
    };
    Diagnostic::new(
        ErrorCode::L0409,
        format!("`{name}` takes nothing in brackets"),
        Label::new(span, "not an array type"),
        help,
    )
}

pub(in crate::semantic) fn array_too_large(ty: &Type, size: Option<u64>, span: Span) -> Diagnostic {
    let size = size.map_or_else(
        || format!("more than {}", u64::MAX),
        |size| size.to_string(),
    );
    Diagnostic::new(
        ErrorCode::L0410,
        format!("`{ty}` takes {size} bytes, more than an array can hold"),
        Label::new(span, "too large for the call stack"),
        Help::text(format!(
            "an array lives on the call stack and holds at most {MAX_ARRAY_SIZE} bytes (1 MiB); \
             use fewer or smaller elements, or keep the data in a `list`, which phase 3 adds"
        )),
    )
}

/// An index known while compiling that falls outside its array.
pub(in crate::semantic) fn index_out_of_range(index: i128, length: u64, span: Span) -> Diagnostic {
    let last = length.saturating_sub(1);
    let help = if index < 0 {
        Help::text(format!(
            "indexes count up from 0, and there are no negative ones; the last element is at {last}"
        ))
    } else {
        Help::text(format!(
            "the array has {length} elements, at indexes 0 to {last}"
        ))
    };
    Diagnostic::new(
        ErrorCode::L0411,
        format!("index {index} is out of range for an array of length {length}"),
        Label::new(
            span,
            "the program would stop here every time it ran this line",
        ),
        help,
    )
}

/// `array(value)` where nothing says which array type it builds.
pub(in crate::semantic) fn fill_without_type(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0412,
        "`array(value)` needs to know which array to build",
        Label::new(span, "its length comes from the type of the place it goes"),
        Help::text(
            "write the type where the array goes, as in `variable array[integer64, 10] values = array(0)`",
        ),
    )
}

pub(in crate::semantic) fn fill_argument_count(found: usize, call: Span) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`array` takes 1 argument, but {found} {given} given"),
        Label::new(call, "`array(value)` sets every element to one value"),
        Help::text("write `array(value)`; the type where it goes gives the length"),
    )
}

pub(in crate::semantic) fn length_argument_count(found: usize, call: Span) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`length` takes 1 argument, but {found} {given} given"),
        Label::new(call, "`length` measures one value"),
        Help::text("write `length(value)`"),
    )
}

pub(in crate::semantic) fn not_indexable(found: &Type, span: Span) -> Diagnostic {
    operator_type(
        format!(
            "only arrays can be indexed, and this is {}",
            described(found)
        ),
        span,
        "not an array".to_owned(),
        "index an array, as in `values[0]`",
    )
}

pub(in crate::semantic) fn index_not_integer(found: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!("an index is an integer, but this is {}", described(found)),
        Label::new(span, "not an integer"),
        Help::text("index with a whole number from 0 up to the length, without it"),
    )
}

pub(in crate::semantic) fn no_length(found: &Type, span: Span) -> Diagnostic {
    operator_type(
        format!(
            "`length` counts the elements of an array or the bytes of a text, but this is {}",
            described(found)
        ),
        span,
        "has no length".to_owned(),
        "pass an array or a `string`",
    )
}

/// An array interpolated into text, which holds numbers, `boolean` values and text.
pub(in crate::semantic) fn array_in_text(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        "an array cannot be put into text",
        Label::new(span, "this is an array"),
        Help::text("put its elements in one by one, as in `{values[0]}`, or print them in a loop"),
    )
}

/// `for` over a value that is neither `range(start, end)` nor an array. `source` is the value as written.
pub(in crate::semantic) fn not_iterable(found: &Type, span: Span, source: &str) -> Diagnostic {
    let help = match found {
        Type::Integer(_) => Help::replace(
            "to count up to a number, loop over a range",
            span,
            format!("range(0, {source})"),
        ),
        Type::Boolean | Type::String | Type::Array(_) => {
            Help::text("loop over `range(start, end)` or over an array")
        }
    };
    Diagnostic::new(
        ErrorCode::L0405,
        format!(
            "`for` loops over a range or an array, but this is {}",
            described(found)
        ),
        Label::new(span, "not a range or an array"),
        help,
    )
}

/// An element passed with `variable`, where only a whole binding can be.
pub(in crate::semantic) fn element_reference(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0504,
        "an element cannot be passed with `variable`",
        Label::new(span, "only a whole binding can be"),
        Help::text(
            "pass the whole array with `variable`, or copy the element into a `variable`, pass that, and store it back",
        ),
    )
}

/// An assignment to an element of a value that is not stored in a binding, as in `make()[0] = 1`.
pub(in crate::semantic) fn element_of_value(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0308,
        "only a binding, or an element inside one, can be assigned",
        Label::new(span, "not a binding"),
        Help::text("store the value in a `variable` first, then assign to its elements"),
    )
}

/// A top-level constant declared with an array type.
pub(in crate::semantic) fn array_constant(ty: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0210,
        "a top-level constant cannot hold an array",
        Label::new(ty, "an array type"),
        Help::text("declare the array inside the function that uses it"),
    )
}
