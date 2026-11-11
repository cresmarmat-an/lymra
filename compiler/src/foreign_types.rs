//! Type names from other languages, and the Lymra type each one stands for.

/// The Lymra type a foreign type name stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Translation {
    pub lymra: &'static str,
    /// False for a name such as `int`, whose size depends on the language it comes from.
    pub is_exact: bool,
}

/**
 * Looks up a type name that programmers bring from other languages. Lymra
 * spells its types as whole words, so `i64` is `integer64` and `bool` is
 * `boolean`.
 */
pub(crate) fn translate_type(foreign: &str) -> Option<Translation> {
    let exact = |lymra| {
        Some(Translation {
            lymra,
            is_exact: true,
        })
    };
    let default_size = |lymra| {
        Some(Translation {
            lymra,
            is_exact: false,
        })
    };
    match foreign {
        "i8" => exact("integer8"),
        "i16" => exact("integer16"),
        "i32" => exact("integer32"),
        "i64" => exact("integer64"),
        "u8" | "byte" => exact("unsigned8"),
        "u16" => exact("unsigned16"),
        "u32" => exact("unsigned32"),
        "u64" => exact("unsigned64"),
        "f32" => exact("float32"),
        "f64" | "double" => exact("float64"),
        "bool" => exact("boolean"),
        "str" => exact("string"),
        "int" => default_size("integer64"),
        "uint" => default_size("unsigned64"),
        "float" => default_size("float64"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sized_names_translate_exactly() {
        let translation = translate_type("u8").expect("`u8` is a known foreign type");
        assert_eq!(
            (translation.lymra, translation.is_exact),
            ("unsigned8", true)
        );
        assert_eq!(
            translate_type("double").map(|found| found.lymra),
            Some("float64")
        );
        assert_eq!(
            translate_type("bool").map(|found| found.lymra),
            Some("boolean")
        );
    }

    #[test]
    fn names_without_a_size_translate_to_the_default_size() {
        let translation = translate_type("int").expect("`int` is a known foreign type");
        assert_eq!(
            (translation.lymra, translation.is_exact),
            ("integer64", false)
        );
    }

    #[test]
    fn lymra_names_and_user_names_are_left_alone() {
        for name in ["integer64", "boolean", "string", "list", "Frog", "count"] {
            assert_eq!(translate_type(name), None, "for `{name}`");
        }
    }
}
