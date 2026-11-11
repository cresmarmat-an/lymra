//! The escape rules of text literals, shared by the lexer and the parser.

/// The character an escape letter stands for, or `None` if `\letter` is not an escape.
pub(super) fn escape_value(letter: char) -> Option<char> {
    match letter {
        'n' => Some('\n'),
        't' => Some('\t'),
        'r' => Some('\r'),
        '0' => Some('\0'),
        '\\' => Some('\\'),
        '"' => Some('"'),
        _ => None,
    }
}

/**
 * Converts the source of one text segment into the characters it stands for.
 *
 * Escapes are replaced and doubled braces become single ones. The lexer has
 * already reported anything malformed, so unknown escapes are kept as written.
 */
pub fn unescape(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut characters = raw.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some(letter) => match escape_value(letter) {
                    Some(value) => text.push(value),
                    None => text.extend(['\\', letter]),
                },
                None => text.push('\\'),
            },
            '{' | '}' => {
                text.push(character);
                characters.next_if_eq(&character);
            }
            _ => text.push(character),
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_become_their_characters() {
        assert_eq!(unescape(r#"a\nb\t\"c\"\\"#), "a\nb\t\"c\"\\");
        assert_eq!(unescape(r"nul\0"), "nul\0");
    }

    #[test]
    fn doubled_braces_become_single() {
        assert_eq!(unescape("{{x}} and {{{{"), "{x} and {{");
    }

    #[test]
    fn malformed_input_is_kept_as_written() {
        assert_eq!(unescape(r"\q"), r"\q");
        assert_eq!(unescape("a}b"), "a}b");
        assert_eq!(unescape("end\\"), "end\\");
    }
}
