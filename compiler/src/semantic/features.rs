//! Parts of edition 0.1 that this compiler does not implement yet, and the roadmap phase that adds each.

use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::source::Span;

/// Built-in functions the spec describes, with the phase that implements each.
const FUTURE_FUNCTIONS: [(&str, u8); 12] = [
    ("bytes", 3),
    ("slice", 3),
    ("list", 3),
    ("stack", 3),
    ("push", 3),
    ("pop", 3),
    ("top", 3),
    ("filter", 3),
    ("sort", 3),
    ("pool", 5),
    ("insert", 5),
    ("remove", 5),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Feature {
    Floats,
    CustomTypes,
    Choices,
    Fields,
    NamedArguments,
    TextComparison,
    TextBuilding,
    Lists,
    ForEach,
    RangeValues,
    CodeArguments,
    Modules,
    Pools,
    /// A built-in function from the spec, with the phase that adds it.
    Function {
        name: &'static str,
        phase: u8,
    },
}

impl Feature {
    /// The built-in function `name` that a later phase adds, if there is one.
    pub fn future_function(name: &str) -> Option<Feature> {
        FUTURE_FUNCTIONS
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|&(name, phase)| Feature::Function { name, phase })
    }

    /// The type name `name` belongs to a later phase, if it does.
    pub fn future_type(name: &str) -> Option<Feature> {
        match name {
            "float32" | "float64" => Some(Feature::Floats),
            "list" | "stack" | "view" => Some(Feature::Lists),
            "pool" | "handle" => Some(Feature::Pools),
            _ => None,
        }
    }

    fn message(self) -> String {
        let subject = match self {
            Self::Floats => "`float32` and `float64` are",
            Self::CustomTypes => "`type` declarations are",
            Self::Choices => "choices and `match` are",
            Self::Fields => "fields are",
            Self::NamedArguments => "named arguments are",
            Self::TextComparison => "comparing text is",
            Self::TextBuilding => {
                return "text with interpolation can only be given to `print` for now".to_owned();
            }
            Self::Lists => "`list`, `stack` and `view` are",
            Self::ForEach => {
                return "`for` can only loop over `range` and arrays for now".to_owned();
            }
            Self::RangeValues => {
                return "`range` can only be used directly in a `for` loop for now".to_owned();
            }
            Self::CodeArguments => "`with` blocks are",
            Self::Modules => "modules are",
            Self::Pools => "pools are",
            Self::Function { name, .. } => return format!("`{name}` is not available yet"),
        };
        format!("{subject} not available yet")
    }

    fn phase(self) -> u8 {
        match self {
            Self::Floats
            | Self::CustomTypes
            | Self::Choices
            | Self::Fields
            | Self::NamedArguments
            | Self::TextComparison => 2,
            Self::TextBuilding
            | Self::Lists
            | Self::ForEach
            | Self::RangeValues
            | Self::CodeArguments => 3,
            Self::Modules => 4,
            Self::Pools => 5,
            Self::Function { phase, .. } => phase,
        }
    }

    pub fn diagnostic(self, span: Span) -> Diagnostic {
        Diagnostic::new(
            ErrorCode::L0309,
            self.message(),
            Label::new(
                span,
                format!("the compiler adds this in roadmap phase {}", self.phase()),
            ),
            Help::text(
                "write this part another way for now; the roadmap in the README lists what each phase adds",
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn future_functions_carry_their_phase() {
        assert_eq!(
            Feature::future_function("push"),
            Some(Feature::Function {
                name: "push",
                phase: 3
            })
        );
        assert_eq!(Feature::future_function("print"), None);
    }

    #[test]
    fn messages_name_the_phase() {
        let diagnostic = Feature::Floats.diagnostic(Span::new(0, 7));
        assert_eq!(
            diagnostic.message,
            "`float32` and `float64` are not available yet"
        );
        assert_eq!(
            diagnostic.primary.text,
            "the compiler adds this in roadmap phase 2"
        );
    }
}
