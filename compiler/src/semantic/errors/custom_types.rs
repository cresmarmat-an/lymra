//! The wording of the errors about types the program declares with `type`.

use super::{described, named};
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::english;
use crate::ir::Type;
use crate::source::Span;

/// A field name that the type does not have, in a construction or after a `.`.
pub(in crate::semantic) fn unknown_field(
    type_name: &str,
    field: &str,
    span: Span,
    suggestion: Option<&str>,
) -> Diagnostic {
    let help = match suggestion {
        Some(name) => Help::replace(format!("did you mean `{name}`?"), span, name),
        None => Help::text(format!(
            "use one of the fields the declaration of `{type_name}` lists"
        )),
    };
    Diagnostic::new(
        ErrorCode::L0413,
        format!("`{type_name}` has no field `{field}`"),
        Label::new(span, "not a field of this type"),
        help,
    )
}

/// `.field` on a value whose type has no fields.
pub(in crate::semantic) fn no_fields(found: &Type, object: Span, field: &str) -> Diagnostic {
    let help = match found {
        Type::Array(_) => {
            "an array has elements, not fields; to read one, index it, as in `values[0]`".to_owned()
        }
        Type::List(_) => {
            "a list has elements, not fields; to read one, index it, as in `values[0]`".to_owned()
        }
        Type::Choice(choice) => match choice
            .variants
            .iter()
            .find(|variant| variant.field(field).is_some())
        {
            Some(variant) => format!(
                "`{field}` belongs to the variant `{}`; read it in a `match`, as in `case {}({field})`",
                variant.name, variant.name
            ),
            None => {
                "a choice holds fields only in its variants; read them in a `match` case".to_owned()
            }
        },
        Type::Integer(_) | Type::Float(_) | Type::Boolean | Type::String | Type::Custom(_) => {
            format!("only a value of a type declared with `type` has fields such as `{field}`")
        }
    };
    Diagnostic::new(
        ErrorCode::L0413,
        format!("this is {}, which has no fields", described(found)),
        Label::new(object, "has no fields"),
        Help::text(help),
    )
}

/// A construction argument without a field name. `field` is the field in that position, if any.
pub(in crate::semantic) fn unnamed_field(
    type_name: &str,
    field: Option<&str>,
    argument: Span,
    value: &str,
) -> Diagnostic {
    let help = match field {
        Some(field) => Help::replace(
            "name the field it is for",
            argument,
            format!("{field} = {value}"),
        ),
        None => Help::text(format!(
            "`{type_name}` has no more fields; remove this value"
        )),
    };
    Diagnostic::new(
        ErrorCode::L0414,
        format!("the fields of {} are given by name", named(type_name)),
        Label::new(argument, "a value without a field name"),
        help,
    )
}

pub(in crate::semantic) fn field_given_twice(
    field: &str,
    span: Span,
    previous: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0414,
        format!("the field `{field}` is given twice"),
        Label::new(span, "given again here"),
        Help::text("give each field once"),
    )
    .with_secondary(Label::new(previous, "first given here"))
}

/// A construction that leaves fields out. `missing` lists them in the order the type declares them.
pub(in crate::semantic) fn missing_fields(
    type_name: &str,
    missing: &[&str],
    span: Span,
) -> Diagnostic {
    let quoted: Vec<String> = missing.iter().map(|field| format!("`{field}`")).collect();
    let additions = missing
        .iter()
        .map(|field| format!("{field} = ..."))
        .collect::<Vec<_>>()
        .join(", ");
    let (verb, label) = if missing.len() == 1 {
        ("is", "this leaves out a field")
    } else {
        ("are", "this leaves out fields")
    };
    Diagnostic::new(
        ErrorCode::L0414,
        format!(
            "every field of {} has to be given; {} {verb} missing",
            named(type_name),
            english::listed(&quoted)
        ),
        Label::new(span, label),
        Help::text(format!(
            "give a value for each, as in `{additions}`; Lymra never fills in a field by itself"
        )),
    )
}

/// A top-level constant whose value holds other values: an array or a value of a declared type.
pub(in crate::semantic) fn compound_constant(found: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0210,
        format!("a top-level constant cannot hold {}", described(found)),
        Label::new(
            span,
            "a top-level constant holds a number, a `boolean` or text",
        ),
        Help::text("declare it inside the function that uses it"),
    )
}
