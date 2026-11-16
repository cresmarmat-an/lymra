//! The wording of the errors about `bit_and`, `bit_or`, `bit_xor`, `bit_not`, `shift_left` and `shift_right`.

use super::described;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::{IntegerType, Type};
use crate::source::Span;

/// A bit function called with other than its `parameters`.
pub(in crate::semantic) fn bits_argument_count(
    function: &str,
    parameters: &[&str],
    found: usize,
    call: Span,
) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    let takes = match parameters.len() {
        1 => "1 argument".to_owned(),
        count => format!("{count} arguments"),
    };
    let label = match parameters {
        ["value"] => "it takes the number whose bits it flips",
        ["value", "count"] => "it takes the number to shift and how many bits to shift it by",
        _ => "it takes the two numbers whose bits it combines",
    };
    Diagnostic::new(
        ErrorCode::L0501,
        format!("`{function}` takes {takes}, but {found} {given} given"),
        Label::new(call, label),
        Help::text(format!("write `{function}({})`", parameters.join(", "))),
    )
}

/// An operand of a bit function that is not an integer.
pub(in crate::semantic) fn bit_operand_type(function: &str, ty: &Type, span: Span) -> Diagnostic {
    let help = match ty {
        Type::Boolean => "for booleans, use `and`, `or` and `not`",
        Type::Float(_) => "convert it to an integer type first, as in `integer64(value)`",
        _ => "bit functions work on single integers; work on the integers it holds one by one",
    };
    Diagnostic::new(
        ErrorCode::L0405,
        format!(
            "`{function}` works on the bits of integers, but this is {}",
            described(ty)
        ),
        Label::new(span, "not an integer"),
        Help::text(help),
    )
}

/// A shift by a count, known while compiling, outside the bits of a value of type `ty`.
pub(in crate::semantic) fn shift_count_out_of_range(
    function: &str,
    count: i128,
    ty: IntegerType,
    span: Span,
) -> Diagnostic {
    let bits = ty.bits();
    Diagnostic::new(
        ErrorCode::L0416,
        format!(
            "`{function}` can shift an `{}` by 0 to {} bits, not {count}",
            ty.name(),
            bits - 1
        ),
        Label::new(span, "outside the bits of the value"),
        Help::text(format!(
            "shift by a count from 0 to {}; a value with every bit shifted out would be written as 0, or as -1 for a negative value shifted right",
            bits - 1
        )),
    )
}
