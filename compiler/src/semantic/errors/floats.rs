//! The wording of the errors about floats.

use super::described;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::{FloatType, Type};
use crate::source::Span;

/// The largest finite value of each float type, roughly, as messages give it.
fn largest(ty: FloatType) -> &'static str {
    match ty {
        FloatType::Float32 => "3.4e38",
        FloatType::Float64 => "1.8e308",
    }
}

/// A float literal, or a result known while compiling, beyond the largest float of its type.
pub(in crate::semantic) fn float_too_large(ty: FloatType, span: Span) -> Diagnostic {
    let help = match ty {
        FloatType::Float32 => {
            "a `float32` goes up to about 3.4e38; use `float64`, which goes up to about 1.8e308"
                .to_owned()
        }
        FloatType::Float64 => format!(
            "a `float64` goes up to about {}; scale the values down, for example by measuring in larger units",
            largest(ty)
        ),
    };
    Diagnostic::new(
        ErrorCode::L0404,
        format!(
            "this is beyond the largest `{}`, so it would be infinity",
            ty.name()
        ),
        Label::new(span, format!("larger than about {}", largest(ty))),
        Help::text(help),
    )
}

/// A float literal that is not zero but rounds to zero in its type.
pub(in crate::semantic) fn float_too_small(ty: FloatType, span: Span) -> Diagnostic {
    let help = match ty {
        FloatType::Float32 => {
            "a `float32` above zero is at least about 1.4e-45; use `float64`, or write 0.0"
        }
        FloatType::Float64 => {
            "a `float64` above zero is at least about 4.9e-324; write 0.0, or scale the values up"
        }
    };
    Diagnostic::new(
        ErrorCode::L0404,
        format!(
            "this is too close to zero for a `{}`, so it would be 0.0",
            ty.name()
        ),
        Label::new(span, "rounds to zero"),
        Help::text(help),
    )
}

/// `%` between floats, which Lymra leaves out: it would need a library function C does not build in.
pub(in crate::semantic) fn float_remainder(ty: FloatType, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0405,
        format!(
            "`%` works on integers, and these are `{}` values",
            ty.name()
        ),
        Label::new(span, "a remainder of floats"),
        Help::text(
            "convert to an integer type first, as in `integer64(value) % 3`, if whole numbers are meant",
        ),
    )
}

/// A conversion of a value known while compiling that can never fit the target type.
pub(in crate::semantic) fn conversion_never_fits(
    shown: &str,
    target: &Type,
    span: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0404,
        format!("{shown} does not fit {}", described(target)),
        Label::new(span, "known now, and outside the target type"),
        Help::text(
            "use a value the target type holds; the program would stop here every time it ran this line",
        ),
    )
}
