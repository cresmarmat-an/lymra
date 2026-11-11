//! The two naming styles of Lymra: checking a name against one, and rewriting a name into one.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Case {
    /// `snake_case`: lowercase words joined by single underscores.
    Snake,
    /// `PascalCase`: words run together, each starting with a capital.
    Pascal,
}

impl Case {
    pub fn name(self) -> &'static str {
        match self {
            Self::Snake => "snake_case",
            Self::Pascal => "PascalCase",
        }
    }

    pub fn matches(self, name: &str) -> bool {
        match self {
            Self::Snake => {
                let is_plain =
                    |character: char| character.is_ascii_lowercase() || character.is_ascii_digit();
                name.starts_with(|first: char| first.is_ascii_lowercase())
                    && name
                        .chars()
                        .all(|character| is_plain(character) || character == '_')
                    && !name.ends_with('_')
                    && !name.contains("__")
            }
            Self::Pascal => {
                name.starts_with(|first: char| first.is_ascii_uppercase())
                    && name
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric())
            }
        }
    }

    /// Rewrites `name` in this style. The result is empty when the name has no letters or digits.
    pub fn convert(self, name: &str) -> String {
        let words = words(name);
        match self {
            Self::Snake => words.join("_"),
            Self::Pascal => words.iter().map(|word| capitalize(word)).collect(),
        }
    }
}

/**
 * Splits a name into lowercase words, whatever style it was written in.
 *
 * A new word starts after an underscore, at a capital that follows a
 * lowercase letter or digit (`getArea`), and at the last capital of a run of
 * capitals when a lowercase letter follows (`HTTPServer` is `http`, `server`).
 */
fn words(name: &str) -> Vec<String> {
    let characters: Vec<char> = name.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();
    for (index, &character) in characters.iter().enumerate() {
        if character == '_' {
            flush(&mut current, &mut words);
            continue;
        }
        let previous = index
            .checked_sub(1)
            .and_then(|before| characters.get(before))
            .copied();
        let next = characters.get(index + 1).copied();
        let after_lowercase =
            previous.is_some_and(|before| before.is_ascii_lowercase() || before.is_ascii_digit());
        let ends_capital_run = previous.is_some_and(|before| before.is_ascii_uppercase())
            && next.is_some_and(|after| after.is_ascii_lowercase());
        if character.is_ascii_uppercase() && (after_lowercase || ends_capital_run) {
            flush(&mut current, &mut words);
        }
        current.push(character.to_ascii_lowercase());
    }
    flush(&mut current, &mut words);
    words
}

fn flush(current: &mut String, words: &mut Vec<String>) {
    if !current.is_empty() {
        words.push(std::mem::take(current));
    }
}

fn capitalize(word: &str) -> String {
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_is_lowercase_words_and_single_underscores() {
        for name in [
            "x",
            "count",
            "get_area",
            "vec3",
            "http_2_server",
            "integer64",
        ] {
            assert!(Case::Snake.matches(name), "`{name}` is snake_case");
        }
        for name in [
            "getArea",
            "Count",
            "_private",
            "trailing_",
            "double__underscore",
            "MAX_SIZE",
            "",
            "2fast",
        ] {
            assert!(!Case::Snake.matches(name), "`{name}` is not snake_case");
        }
    }

    #[test]
    fn pascal_case_starts_with_a_capital_and_has_no_underscores() {
        for name in ["T", "Frog", "HttpServer", "HTTPServer", "Vec3"] {
            assert!(Case::Pascal.matches(name), "`{name}` is PascalCase");
        }
        for name in ["frog", "Frog_Type", "frogType", ""] {
            assert!(!Case::Pascal.matches(name), "`{name}` is not PascalCase");
        }
    }

    #[test]
    fn names_convert_to_snake_case() {
        let cases = [
            ("getArea", "get_area"),
            ("GetArea", "get_area"),
            ("HTTPServer", "http_server"),
            ("MAX_SIZE", "max_size"),
            ("Vec3D", "vec3_d"),
            ("_private", "private"),
            ("double__underscore_", "double_underscore"),
            ("already_fine", "already_fine"),
        ];
        for (name, expected) in cases {
            assert_eq!(Case::Snake.convert(name), expected, "for `{name}`");
        }
    }

    #[test]
    fn names_convert_to_pascal_case() {
        let cases = [
            ("frog", "Frog"),
            ("frog_type", "FrogType"),
            ("frogType", "FrogType"),
            ("HTTP_server", "HttpServer"),
            ("Frog_Type", "FrogType"),
        ];
        for (name, expected) in cases {
            assert_eq!(Case::Pascal.convert(name), expected, "for `{name}`");
        }
    }

    #[test]
    fn a_name_without_letters_converts_to_nothing() {
        assert_eq!(Case::Snake.convert("__"), "");
    }
}
