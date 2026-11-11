/*!
 * Scans number literals: `42`, `1_000_000`, `0xFF`, `0b1010`, `3.14`, `1.0e-9`.
 *
 * Scanning is a pure function of the text, in two steps: find where the
 * literal ends, taking any letters stuck to it, then check that spelling
 * against the grammar. A malformed number is therefore one error on the
 * whole literal, not a number followed by a confusing second token.
 */

use crate::foreign_types::translate_type;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum NumberValue {
    Integer(u64),
    Float(f64),
}

/// Why a literal was rejected, with the corrected spelling when there is exactly one.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct NumberError {
    pub message: String,
    pub help: String,
    pub replacement: Option<String>,
}

impl NumberError {
    fn new(message: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            help: help.into(),
            replacement: None,
        }
    }

    fn with_replacement(mut self, replacement: impl Into<String>) -> Self {
        self.replacement = Some(replacement.into());
        self
    }
}

pub(super) struct ScannedNumber {
    /// Bytes of source the literal covers, including any malformed tail.
    pub length: usize,
    pub value: Result<NumberValue, NumberError>,
}

/// Scans the literal at the start of `text`, which must begin with an ASCII digit.
pub(super) fn scan_number(text: &str) -> ScannedNumber {
    let length = literal_length(text.as_bytes());
    let literal = text.get(..length).unwrap_or_default();
    ScannedNumber {
        length,
        value: parse_literal(literal),
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The index just past the run of letters, digits and underscores that starts at `from`.
fn word_end(bytes: &[u8], from: usize) -> usize {
    let run = bytes.get(from..).unwrap_or_default();
    from + run.iter().take_while(|&&byte| is_word_byte(byte)).count()
}

fn has_radix_prefix(bytes: &[u8]) -> bool {
    matches!(bytes, [b'0', b'x' | b'X' | b'b' | b'B' | b'o' | b'O', ..])
}

fn literal_length(bytes: &[u8]) -> usize {
    let mut length = word_end(bytes, 0);

    // A dot belongs to the number only when a digit follows, so `1.field` and `1..2` stay separate.
    let digit_after = |index: usize| bytes.get(index).is_some_and(u8::is_ascii_digit);
    if bytes.get(length) == Some(&b'.') && digit_after(length + 1) {
        length = word_end(bytes, length + 1);
    }

    // A sign belongs to the number only right after an exponent marker, as in `1.0e-9`.
    let ends_with_marker = length > 0 && matches!(bytes.get(length - 1), Some(b'e' | b'E'));
    let has_sign = matches!(bytes.get(length), Some(b'+' | b'-'));
    if ends_with_marker && has_sign && digit_after(length + 1) && !has_radix_prefix(bytes) {
        length = word_end(bytes, length + 1);
    }
    length
}

fn parse_literal(literal: &str) -> Result<NumberValue, NumberError> {
    let prefix = literal.get(..2).unwrap_or_default().to_ascii_lowercase();
    match prefix.as_str() {
        "0x" => parse_radix(literal, 16, "hexadecimal", "0-9 and A-F"),
        "0b" => parse_radix(literal, 2, "binary", "0 and 1"),
        "0o" => Err(NumberError::new(
            "octal numbers are not part of Lymra",
            "write the value in decimal, hexadecimal (`0x`) or binary (`0b`)",
        )),
        _ => parse_decimal(literal),
    }
}

fn parse_radix(
    literal: &str,
    radix: u32,
    name: &str,
    digit_names: &str,
) -> Result<NumberValue, NumberError> {
    let (prefix, digits) = literal.split_at(2);
    let lowercase_prefix = prefix.to_ascii_lowercase();
    if prefix != lowercase_prefix {
        let help = format!("write `{lowercase_prefix}`");
        return Err(NumberError::new(
            format!("the number prefix `{prefix}` must be lowercase"),
            help,
        )
        .with_replacement(format!("{lowercase_prefix}{digits}")));
    }
    if digits.is_empty() {
        let help = format!("add {name} digits after `{prefix}`");
        return Err(NumberError::new(
            format!("`{prefix}` has no digits after it"),
            help,
        ));
    }
    if let Some(repaired) = repair_underscores(digits, |digit| digit.is_ascii_hexdigit()) {
        return Err(misplaced_underscore(format!("{prefix}{repaired}")));
    }
    if digits.contains('.') {
        return Err(NumberError::new(
            format!("a {name} number cannot have a fraction"),
            "write a number with a fraction in decimal, for example `1.5`",
        ));
    }
    if let Some(bad) = digits
        .chars()
        .find(|&digit| digit != '_' && !digit.is_digit(radix))
    {
        let help = format!("{name} digits are {digit_names}");
        return Err(NumberError::new(
            format!("`{bad}` is not a {name} digit"),
            help,
        ));
    }
    let clean: String = digits.chars().filter(|&digit| digit != '_').collect();
    u64::from_str_radix(&clean, radix)
        .map(NumberValue::Integer)
        .map_err(|_| too_large())
}

fn parse_decimal(literal: &str) -> Result<NumberValue, NumberError> {
    let (integer, rest) = split_digits(literal);
    let (fraction, rest) = match rest.strip_prefix('.') {
        Some(after_dot) => {
            let (fraction, rest) = split_digits(after_dot);
            (Some(fraction), rest)
        }
        None => (None, rest),
    };
    let exponent = if rest.is_empty() {
        None
    } else {
        Some(parse_exponent(literal, rest)?)
    };

    if let Some(repaired) = repair_underscores(literal, |digit| digit.is_ascii_digit()) {
        return Err(misplaced_underscore(repaired));
    }
    check_leading_zeros(literal, integer)?;

    if fraction.is_none() {
        if let Some(exponent) = exponent {
            return Err(NumberError::new(
                "a float needs a decimal point",
                "write the digits after the point, even when they are zero",
            )
            .with_replacement(format!("{integer}.0e{exponent}")));
        }
        let clean: String = integer.chars().filter(|&digit| digit != '_').collect();
        return clean
            .parse()
            .map(NumberValue::Integer)
            .map_err(|_| too_large());
    }

    let clean: String = literal
        .chars()
        .filter(|&character| character != '_')
        .collect();
    match clean.parse::<f64>() {
        Ok(value) if value.is_finite() => Ok(NumberValue::Float(value)),
        _ => Err(NumberError::new(
            "this float is too large for a `float64`",
            "the largest `float64` is about 1.8e308",
        )),
    }
}

/// Splits `text` after its leading run of digits and underscores.
fn split_digits(text: &str) -> (&str, &str) {
    let end = text.find(|character: char| !character.is_ascii_digit() && character != '_');
    text.split_at(end.unwrap_or(text.len()))
}

/// Parses the tail of a decimal literal as an exponent and returns it without the marker.
fn parse_exponent<'a>(literal: &str, tail: &'a str) -> Result<&'a str, NumberError> {
    let Some(exponent) = tail.strip_prefix(['e', 'E']) else {
        return Err(suffix_error(literal, tail));
    };
    let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
    if digits.is_empty()
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'_')
    {
        return Err(suffix_error(literal, tail));
    }
    if tail.starts_with('E') {
        let number = literal.strip_suffix(tail).unwrap_or(literal);
        return Err(
            NumberError::new("the exponent marker `E` must be lowercase", "write `e`")
                .with_replacement(format!("{number}e{exponent}")),
        );
    }
    Ok(exponent)
}

fn suffix_error(literal: &str, tail: &str) -> NumberError {
    let number = literal.strip_suffix(tail).unwrap_or(literal);
    // A suffix such as `u8` is how other languages give a number its type.
    if let Some(suffix) = translate_type(tail).filter(|translation| translation.is_exact) {
        let help = format!(
            "write the type in the declaration, for example `constant {} value = {number}`",
            suffix.lymra
        );
        return NumberError::new("a number cannot carry a type suffix", help)
            .with_replacement(number);
    }
    let help = format!(
        "a number cannot run straight into letters: remove `{tail}` or put a space or an operator before it"
    );
    NumberError::new(format!("`{literal}` is not a number"), help)
}

/// Removes every underscore that is not between two digits. Returns `None` when all are well placed.
fn repair_underscores(text: &str, is_digit: impl Fn(char) -> bool) -> Option<String> {
    let characters: Vec<char> = text.chars().collect();
    let is_separator = |index: usize| {
        let before = index
            .checked_sub(1)
            .and_then(|previous| characters.get(previous));
        let after = characters.get(index + 1);
        before.is_some_and(|&digit| is_digit(digit)) && after.is_some_and(|&digit| is_digit(digit))
    };
    let repaired: String = characters
        .iter()
        .enumerate()
        .filter(|&(index, &character)| character != '_' || is_separator(index))
        .map(|(_, &character)| character)
        .collect();
    (repaired != text).then_some(repaired)
}

fn misplaced_underscore(repaired: String) -> NumberError {
    NumberError::new(
        "misplaced `_` in a number",
        "use a single `_` between digits, as in `1_000_000`",
    )
    .with_replacement(repaired)
}

fn check_leading_zeros(literal: &str, integer: &str) -> Result<(), NumberError> {
    let significant = integer.trim_start_matches(['0', '_']);
    let has_leading_zero = integer.len() > 1 && integer.starts_with('0');
    if !has_leading_zero {
        return Ok(());
    }
    let tail = literal.strip_prefix(integer).unwrap_or_default();
    let repaired_integer = if significant.is_empty() {
        "0"
    } else {
        significant
    };
    Err(NumberError::new(
        "a decimal number cannot start with `0`",
        "remove the leading zeros; they do not make the number octal",
    )
    .with_replacement(format!("{repaired_integer}{tail}")))
}

fn too_large() -> NumberError {
    NumberError::new(
        "this integer is too large for any integer type",
        format!("the largest integer, an `unsigned64`, is {}", u64::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> NumberValue {
        let scanned = scan_number(text);
        assert_eq!(scanned.length, text.len(), "`{text}` should be one literal");
        scanned
            .value
            .unwrap_or_else(|error| panic!("`{text}` was rejected: {}", error.message))
    }

    fn error(text: &str) -> NumberError {
        let scanned = scan_number(text);
        assert_eq!(scanned.length, text.len(), "`{text}` should be one literal");
        scanned.value.expect_err("literal should be rejected")
    }

    #[test]
    fn integers_in_every_radix() {
        assert_eq!(value("42"), NumberValue::Integer(42));
        assert_eq!(value("0"), NumberValue::Integer(0));
        assert_eq!(value("1_000_000"), NumberValue::Integer(1_000_000));
        assert_eq!(value("0xFF"), NumberValue::Integer(255));
        assert_eq!(value("0xff_ff"), NumberValue::Integer(65_535));
        assert_eq!(value("0b1010"), NumberValue::Integer(10));
        assert_eq!(
            value("18446744073709551615"),
            NumberValue::Integer(u64::MAX)
        );
    }

    #[test]
    fn floats_with_and_without_exponents() {
        assert_eq!(value("2.75"), NumberValue::Float(2.75));
        assert_eq!(value("1.0e-9"), NumberValue::Float(1.0e-9));
        assert_eq!(value("2.5e+3"), NumberValue::Float(2500.0));
        assert_eq!(value("6.02e23"), NumberValue::Float(6.02e23));
        assert_eq!(value("1_000.5"), NumberValue::Float(1000.5));
        assert_eq!(value("0.5"), NumberValue::Float(0.5));
    }

    #[test]
    fn a_literal_stops_before_text_that_is_not_part_of_it() {
        assert_eq!(scan_number("1.field").length, 1);
        assert_eq!(scan_number("1..5").length, 1);
        assert_eq!(scan_number("7)").length, 1);
        assert_eq!(scan_number("3-1").length, 1);
        assert_eq!(scan_number("0x1e+5").length, 4);
        assert_eq!(scan_number("1.5e-x").length, 4);
    }

    #[test]
    fn corrected_spellings_are_offered() {
        assert_eq!(error("0XFF").replacement.as_deref(), Some("0xFF"));
        assert_eq!(error("007").replacement.as_deref(), Some("7"));
        assert_eq!(error("000").replacement.as_deref(), Some("0"));
        assert_eq!(error("00.5").replacement.as_deref(), Some("0.5"));
        assert_eq!(error("1e9").replacement.as_deref(), Some("1.0e9"));
        assert_eq!(error("1e-9").replacement.as_deref(), Some("1.0e-9"));
        assert_eq!(error("1.5E3").replacement.as_deref(), Some("1.5e3"));
        assert_eq!(error("1__000").replacement.as_deref(), Some("1000"));
        assert_eq!(error("100_").replacement.as_deref(), Some("100"));
        assert_eq!(error("0x_FF").replacement.as_deref(), Some("0xFF"));
        assert_eq!(error("1.5e_3").replacement.as_deref(), Some("1.5e3"));
        assert_eq!(error("1_.5").replacement.as_deref(), Some("1.5"));
        assert_eq!(error("42u8").replacement.as_deref(), Some("42"));
    }

    #[test]
    fn malformed_literals_are_rejected_whole() {
        assert_eq!(error("123abc").message, "`123abc` is not a number");
        assert_eq!(error("0o17").message, "octal numbers are not part of Lymra");
        assert_eq!(error("0x").message, "`0x` has no digits after it");
        assert_eq!(error("0b102").message, "`2` is not a binary digit");
        assert_eq!(error("0xFG").message, "`G` is not a hexadecimal digit");
        assert_eq!(
            error("0x1.8").message,
            "a hexadecimal number cannot have a fraction"
        );
        assert_eq!(error("1.5e").message, "`1.5e` is not a number");
        assert_eq!(
            error("18446744073709551616").message,
            "this integer is too large for any integer type"
        );
        assert_eq!(
            error("1.0e999").message,
            "this float is too large for a `float64`"
        );
    }
}
