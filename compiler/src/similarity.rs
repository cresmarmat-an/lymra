//! "Did you mean" lookups for misspelled words.

/**
 * Edit distance between two words, where swapping two adjacent characters
 * counts as one edit. Typos such as `whlie` are swaps, so plain Levenshtein
 * distance would score them as two edits and miss them.
 */
pub fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();

    // Three rolling rows of the distance table: two back, one back, and current.
    let mut two_back = vec![0; right.len() + 1];
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];

    for (row, &left_char) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, &right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != right_char);
            let mut best = substitution
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
            let is_swap = row > 0
                && column > 0
                && left_char == right[column - 1]
                && left[row - 1] == right_char;
            if is_swap {
                best = best.min(two_back[column - 1] + 1);
            }
            current[column + 1] = best;
        }
        std::mem::swap(&mut two_back, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// The candidate closest to `word`, if it is close enough to be a likely typo.
pub fn closest_match<'a>(
    word: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    // Allow one edit per three characters, so short words do not match everything.
    let limit = (word.chars().count() / 3).max(1);
    candidates
        .into_iter()
        .map(|candidate| (edit_distance(word, candidate), candidate))
        .filter(|(distance, _)| *distance <= limit)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_counts_single_edits() {
        assert_eq!(edit_distance("while", "while"), 0);
        assert_eq!(edit_distance("whle", "while"), 1);
        assert_eq!(edit_distance("whiles", "while"), 1);
        assert_eq!(edit_distance("whyle", "while"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn a_swap_of_neighbours_is_one_edit() {
        assert_eq!(edit_distance("whlie", "while"), 1);
        assert_eq!(edit_distance("retrun", "return"), 1);
        assert_eq!(edit_distance("fucntion", "function"), 1);
    }

    #[test]
    fn closest_match_rejects_distant_words() {
        let keywords = ["while", "return", "function"];
        assert_eq!(closest_match("whlie", keywords), Some("while"));
        assert_eq!(
            closest_match("optimise", ["optimize", "edition"]),
            Some("optimize")
        );
        assert_eq!(closest_match("string", keywords), None);
        assert_eq!(closest_match("x", keywords), None);
    }
}
