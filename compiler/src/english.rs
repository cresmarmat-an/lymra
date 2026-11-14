//! English wording shared by the compiler's messages.

/// Words that start with a vowel letter but the sound of a consonant, as in "a `User`".
const CONSONANT_SOUNDS: [&str; 2] = ["eu", "one"];

/// Words that start with a consonant letter but a vowel sound, as in "an `Hour`".
const VOWEL_SOUNDS: [&str; 5] = ["heir", "honest", "honor", "honour", "hour"];

/**
 * "a" or "an", whichever goes before `word`. The choice follows the sound
 * the word starts with, judged from its spelling: a `u` followed by one
 * consonant and a vowel sounds like "you", as in `unit` and `user`, while
 * `unsigned8` starts with a vowel sound. Spelling only approximates sound,
 * so a rare word gets the other article.
 */
pub fn article(word: &str) -> &'static str {
    let word = word.to_ascii_lowercase();
    if VOWEL_SOUNDS.iter().any(|start| word.starts_with(start)) {
        return "an";
    }
    if CONSONANT_SOUNDS.iter().any(|start| word.starts_with(start)) {
        return "a";
    }
    match word.as_bytes() {
        [b'u', second, third, ..] if !is_vowel(*second) && is_vowel(*third) => "a",
        [first, ..] if is_vowel(*first) => "an",
        _ => "a",
    }
}

fn is_vowel(letter: u8) -> bool {
    matches!(letter, b'a' | b'e' | b'i' | b'o' | b'u')
}

/// Items joined as a sentence lists them: "a", "a and b", "a, b and c".
pub fn listed(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vowel_letters_take_an() {
        for word in [
            "integer64",
            "array",
            "Apple",
            "Order",
            "Event",
            "unsigned8",
            "Umbrella",
        ] {
            assert_eq!(article(word), "an", "{word}");
        }
    }

    #[test]
    fn consonant_letters_take_a() {
        for word in ["boolean", "string", "Frog", "Node", ""] {
            assert_eq!(article(word), "a", "{word}");
        }
    }

    #[test]
    fn the_sound_decides_over_the_letter() {
        for word in ["User", "Unit", "Union", "Euro", "OneShot", "usize"] {
            assert_eq!(article(word), "a", "{word}");
        }
        for word in ["Hour", "HonestBroker"] {
            assert_eq!(article(word), "an", "{word}");
        }
    }

    #[test]
    fn lists_read_as_a_sentence() {
        let words = |items: &[&str]| {
            items
                .iter()
                .map(|&item| item.to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(listed(&words(&[])), "");
        assert_eq!(listed(&words(&["`name`"])), "`name`");
        assert_eq!(listed(&words(&["`name`", "`age`"])), "`name` and `age`");
        assert_eq!(
            listed(&words(&["`name`", "`age`", "`home`"])),
            "`name`, `age` and `home`"
        );
    }
}
