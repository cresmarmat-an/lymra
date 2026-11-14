//! The wording of the errors about declarations whose definitions lead back to themselves.

use super::named;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::source::Span;

/// A declaration on the way around a cycle, by name.
#[derive(Clone, Copy, Debug)]
pub(in crate::semantic) enum Declared<'a> {
    Type(&'a str),
    Constant(&'a str),
}

impl<'a> Declared<'a> {
    fn constant(self) -> Option<&'a str> {
        match self {
            Declared::Constant(name) => Some(name),
            Declared::Type(_) => None,
        }
    }
}

/// One step around a cycle: `holder` needs `held`, which it names at `span`.
#[derive(Clone, Copy, Debug)]
pub(in crate::semantic) struct Dependency<'a> {
    pub holder: Declared<'a>,
    pub held: Declared<'a>,
    pub span: Span,
}

impl Dependency<'_> {
    fn described(&self) -> String {
        match (self.holder, self.held) {
            (Declared::Type(holder), Declared::Type(held)) => {
                format!("{} holds {} here", named(holder), named(held))
            }
            (Declared::Type(holder), Declared::Constant(held)) => {
                format!("{} needs `{held}` here", named(holder))
            }
            (Declared::Constant(holder), Declared::Type(held)) => {
                format!("the value of `{holder}` needs {} here", named(held))
            }
            (Declared::Constant(holder), Declared::Constant(held)) => {
                format!("the value of `{holder}` needs `{held}` here")
            }
        }
    }
}

/**
 * A declaration whose definition leads back to itself. `closing` names it
 * again; `earlier` are the steps through other declarations that lead
 * there, if any. Types alone that hold each other are error `L0415`; with
 * a constant on the way, the constant has no value, error `L0210`.
 */
pub(in crate::semantic) fn cycle(
    declaration: Span,
    earlier: &[Dependency<'_>],
    closing: &Dependency<'_>,
) -> Diagnostic {
    let holders = || {
        earlier
            .iter()
            .map(|step| step.holder)
            .chain([closing.holder])
    };
    let constant = [closing.held]
        .into_iter()
        .chain(holders())
        .find_map(Declared::constant);
    let diagnostic = match (constant, closing.held) {
        (Some(constant), _) => depends_on_itself(constant, earlier, closing),
        (None, Declared::Type(name) | Declared::Constant(name)) => {
            contains_itself(name, earlier, closing)
        }
    };
    if earlier.is_empty() {
        return diagnostic.with_secondary(Label::new(declaration, "declared here"));
    }
    earlier.iter().fold(diagnostic, |diagnostic, step| {
        diagnostic.with_secondary(Label::new(step.span, step.described()))
    })
}

/// A type that holds itself, directly or through other types. The fix holds an index instead.
fn contains_itself(name: &str, earlier: &[Dependency<'_>], closing: &Dependency<'_>) -> Diagnostic {
    let label = if earlier.is_empty() {
        format!(
            "{}, which would hold another, without end",
            closing.described()
        )
    } else {
        format!(
            "and {}, so they would hold each other without end",
            closing.described()
        )
    };
    Diagnostic::new(
        ErrorCode::L0415,
        format!("`{name}` contains itself"),
        Label::new(closing.span, label),
        Help::replace(
            format!("keep the `{name}` values in an array and hold the index of one here instead"),
            closing.span,
            "integer64",
        ),
    )
}

/// A constant whose value needs itself, directly or through other declarations.
fn depends_on_itself(
    name: &str,
    earlier: &[Dependency<'_>],
    closing: &Dependency<'_>,
) -> Diagnostic {
    let label = match earlier {
        [] => "used here while it is being computed".to_owned(),
        [_] => format!(
            "and {}, so neither can be worked out first",
            closing.described()
        ),
        _ => format!(
            "and {}, so none of them can be worked out first",
            closing.described()
        ),
    };
    Diagnostic::new(
        ErrorCode::L0210,
        format!("the constant `{name}` depends on itself"),
        Label::new(closing.span, label),
        Help::text(format!(
            "compute `{name}` from values that do not lead back to it"
        )),
    )
}
