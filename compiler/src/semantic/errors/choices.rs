//! The wording of the errors about types the program declares with `choice`, and `match`.

use super::described;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::english;
use crate::ir::{ChoiceType, CustomType, Type};
use crate::source::Span;

/// How a variant is built, with every field to fill in, as in `Circle(radius = ...)`.
fn construction(variant: &CustomType) -> String {
    if variant.fields.is_empty() {
        return variant.name.clone();
    }
    let fields: Vec<String> = variant
        .fields
        .iter()
        .map(|field| format!("{} = ...", field.name))
        .collect();
    format!("{}({})", variant.name, fields.join(", "))
}

/// The `case` that handles a variant and names all its fields, as in `case Rect(width, height)`.
fn case(variant: &CustomType) -> String {
    if variant.fields.is_empty() {
        return format!("case {}", variant.name);
    }
    let fields: Vec<&str> = variant
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    format!("case {}({})", variant.name, fields.join(", "))
}

/// The name of a choice called like a type with fields, as in `Shape(radius = 2)`.
pub(in crate::semantic) fn choice_called(choice: &ChoiceType, span: Span) -> Diagnostic {
    let example = choice
        .variants
        .first()
        .map_or_else(String::new, construction);
    Diagnostic::new(
        ErrorCode::L0507,
        format!(
            "`{}` is a choice, so a value of it is one of its variants",
            choice.name
        ),
        Label::new(span, "not a variant"),
        Help::text(format!("build one of its variants, as in `{example}`")),
    )
}

/// A variant without fields written with brackets, as in `Dot()`. `brackets` is the part to remove.
pub(in crate::semantic) fn fieldless_variant_called(variant: &str, brackets: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0507,
        format!("`{variant}` holds no fields, so it is written without brackets"),
        Label::new(brackets, "nothing goes here"),
        Help::remove(format!("write `{variant}` alone"), brackets),
    )
}

/// `match` on a value that is not a choice.
pub(in crate::semantic) fn match_needs_choice(found: &Type, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0402,
        format!("`match` takes a choice, but this is {}", described(found)),
        Label::new(span, "not a choice"),
        Help::text("tell values of other types apart with `if` and comparisons"),
    )
}

/// What a `case` names instead of a variant of the choice being matched.
pub(in crate::semantic) enum Stranger<'a> {
    /// `case _`, which Lymra does not have.
    Everything,
    /// A variant of another choice.
    Elsewhere { choice: &'a str },
    /// A name that is no variant at all, with the closest variant of this choice.
    Unknown { suggestion: Option<&'a str> },
}

/// A `case` whose name is not a variant of `choice`.
pub(in crate::semantic) fn not_a_variant(
    choice: &str,
    name: &str,
    span: Span,
    stranger: Stranger<'_>,
) -> Diagnostic {
    let (message, label, help) = match stranger {
        Stranger::Everything => (
            "a `case` names one variant; Lymra has no case for every other variant".to_owned(),
            "not a variant".to_owned(),
            Help::text(
                "write a `case` for each variant, so that adding a variant later shows every \
                 `match` that has to handle it",
            ),
        ),
        Stranger::Elsewhere { choice: other } => (
            format!("`{name}` is a variant of `{other}`, not of `{choice}`"),
            format!("not a variant of `{choice}`"),
            Help::text(format!(
                "handle the variants of `{choice}` here, or match a value of `{other}`"
            )),
        ),
        Stranger::Unknown { suggestion } => (
            format!("`{name}` is not a variant of `{choice}`"),
            "not a variant of this choice".to_owned(),
            match suggestion {
                Some(variant) => Help::replace(format!("did you mean `{variant}`?"), span, variant),
                None => Help::text(format!(
                    "name one of the variants the declaration of `{choice}` lists"
                )),
            },
        ),
    };
    Diagnostic::new(ErrorCode::L0401, message, Label::new(span, label), help)
}

/**
 * A `case` that binds a field under its own name where that name is taken
 * already. `suggested` is another name for the binding.
 */
pub(in crate::semantic) fn field_name_taken(
    field: &str,
    span: Span,
    previous: Span,
    suggested: &str,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0207,
        format!("`{field}` is already declared"),
        Label::new(span, "this binds the field under the same name"),
        Help::replace(
            "bind the field under another name; in Lymra one name never hides another",
            span,
            format!("{field} = {suggested}"),
        ),
    )
    .with_secondary(Label::new(previous, "first declared here"))
}

/// A second `case` for one variant.
pub(in crate::semantic) fn variant_handled_twice(
    variant: &str,
    span: Span,
    previous: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0401,
        format!("`{variant}` is handled twice"),
        Label::new(span, "handled again here"),
        Help::text(
            "keep one `case` for it; the first one that matches is the only one that would run",
        ),
    )
    .with_secondary(Label::new(previous, "first handled here"))
}

/// A `match` without a `case` for every variant. `missing` are in the order the choice lists them.
pub(in crate::semantic) fn missing_cases(
    choice: &str,
    missing: &[&CustomType],
    header: Span,
) -> Diagnostic {
    let names: Vec<String> = missing
        .iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect();
    let cases: Vec<String> = missing
        .iter()
        .map(|variant| format!("`{}`", case(variant)))
        .collect();
    let (verb, label) = if missing.len() == 1 {
        ("is", "this leaves out a variant")
    } else {
        ("are", "this leaves out variants")
    };
    Diagnostic::new(
        ErrorCode::L0401,
        format!(
            "a `match` handles every variant of `{choice}`; {} {verb} not handled",
            english::listed(&names)
        ),
        Label::new(header, label),
        Help::text(format!("add {}", english::listed(&cases))),
    )
}
