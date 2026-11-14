//! How Lymra types, names and values are written in C.

use crate::ir::{FloatType, IntegerType, Value};

pub(super) fn integer_type(ty: IntegerType) -> &'static str {
    match ty {
        IntegerType::Integer8 => "int8_t",
        IntegerType::Integer16 => "int16_t",
        IntegerType::Integer32 => "int32_t",
        IntegerType::Integer64 => "int64_t",
        IntegerType::Unsigned8 => "uint8_t",
        IntegerType::Unsigned16 => "uint16_t",
        IntegerType::Unsigned32 => "uint32_t",
        IntegerType::Unsigned64 => "uint64_t",
    }
}

/*
 * Every Lymra name gets a prefix in C. Lymra names are snake_case ASCII, so
 * a prefixed name can never be a C keyword, a C library name, or a name the
 * runtime uses.
 */

pub(super) fn function_name(name: &str) -> String {
    format!("function_{name}")
}

pub(super) fn constant_name(name: &str) -> String {
    format!("constant_{name}")
}

pub(super) fn local_name(name: &str) -> String {
    format!("local_{name}")
}

/// The struct of a declared type. Lymra type names are PascalCase, so the prefix keeps them apart from every other C name.
pub(super) fn custom_type_name(name: &str) -> String {
    format!("type_{name}")
}

/// The member of a declared type's struct that holds the field at `position`.
pub(super) fn field_name(position: usize) -> String {
    format!("field_{position}")
}

/// The struct of a choice, kept apart from other C names by its prefix like a declared type.
pub(super) fn choice_name(name: &str) -> String {
    format!("choice_{name}")
}

/// The member of a choice's struct that says which variant the value holds.
pub(super) const TAG: &str = "tag";

/// The member of a choice's struct that holds the fields of its variants, one struct for each.
pub(super) const VARIANTS: &str = "variants";

/// The struct, inside a choice's union, of the variant at `position`.
pub(super) fn variant_name(position: usize) -> String {
    format!("variant_{position}")
}

/**
 * A C string literal holding exactly the bytes of `text`. Bytes outside
 * printable ASCII are written as three-digit octal escapes, which cannot run
 * into the characters after them the way hexadecimal escapes can. A `?` is
 * escaped so that no `??` sequence can be read as a C trigraph.
 */
pub(super) fn string_literal(text: &str) -> String {
    let mut literal = String::with_capacity(text.len() + 2);
    literal.push('"');
    for byte in text.bytes() {
        match byte {
            b'"' => literal.push_str("\\\""),
            b'\\' => literal.push_str("\\\\"),
            b'?' => literal.push_str("\\?"),
            b'\n' => literal.push_str("\\n"),
            b'\t' => literal.push_str("\\t"),
            b' '..=b'~' => literal.push(char::from(byte)),
            _ => literal.push_str(&format!("\\{byte:03o}")),
        }
    }
    literal.push('"');
    literal
}

/// The fields of a `lymra_string` for `text`, without the type. Usable in a static initializer.
pub(super) fn string_fields(text: &str) -> String {
    format!("{{{}, {}}}", string_literal(text), text.len())
}

/// An integer constant of exactly type `ty`. `value` lies within the range of `ty`.
pub(super) fn integer_literal(value: i128, ty: IntegerType) -> String {
    let minimum = i128::from(i64::MIN);
    let maximum = i128::from(i64::MAX);
    let wide = if value == minimum {
        "INT64_MIN".to_owned()
    } else if value < 0 {
        format!("(-INT64_C({}))", -value)
    } else if value <= maximum && ty != IntegerType::Unsigned64 {
        format!("INT64_C({value})")
    } else {
        format!("UINT64_C({value})")
    };
    match ty {
        IntegerType::Integer64 | IntegerType::Unsigned64 => wide,
        _ => format!("(({}){wide})", integer_type(ty)),
    }
}

/// Text inside a function body.
pub(super) fn text_value(text: &str) -> String {
    format!("((lymra_string){})", string_fields(text))
}

/**
 * A finite float as a C hexadecimal literal, which says exactly which float
 * it is; decimal digits would leave the rounding to the C compiler. A
 * `float32` takes the `f` suffix. Every `float32` is a `float64`, so `bits`
 * are a `float64`'s either way.
 */
pub(super) fn float_literal(bits: u64, ty: FloatType) -> String {
    let sign = if bits >> 63 == 1 { "-" } else { "" };
    let suffix = match ty {
        FloatType::Float32 => "f",
        FloatType::Float64 => "",
    };
    let magnitude = bits & !(1 << 63);
    if magnitude == 0 {
        return format!("{sign}0x0p+0{suffix}");
    }
    let biased = magnitude >> 52;
    let fraction = magnitude & ((1 << 52) - 1);
    // A subnormal has no leading one, and the exponent of the smallest normal.
    let (lead, exponent) = if biased == 0 {
        (0, -1022)
    } else {
        (1, biased as i64 - 1023)
    };
    let digits = format!("{fraction:013x}");
    let digits = digits.trim_end_matches('0');
    let point = if digits.is_empty() {
        String::new()
    } else {
        format!(".{digits}")
    };
    format!("{sign}0x{lead}{point}p{exponent:+}{suffix}")
}

/// A value inside a function body.
pub(super) fn value(value: &Value) -> String {
    match value {
        Value::Integer { value, ty } => integer_literal(*value, *ty),
        Value::Float { bits, ty } => float_literal(*bits, *ty),
        Value::Boolean(true) => "true".to_owned(),
        Value::Boolean(false) => "false".to_owned(),
        Value::Text(text) => text_value(text),
    }
}

/// A value as the initializer of a static constant, where C allows no compound literals.
pub(super) fn static_initializer(value: &Value) -> String {
    match value {
        Value::Text(text) => string_fields(text),
        Value::Integer { .. } | Value::Float { .. } | Value::Boolean(_) => self::value(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_are_written_exactly_in_hexadecimal() {
        let literal = |value: f64, ty| float_literal(value.to_bits(), ty);
        assert_eq!(literal(0.0, FloatType::Float64), "0x0p+0");
        assert_eq!(literal(-0.0, FloatType::Float64), "-0x0p+0");
        assert_eq!(literal(1.0, FloatType::Float64), "0x1p+0");
        assert_eq!(literal(-2.5, FloatType::Float64), "-0x1.4p+1");
        assert_eq!(literal(0.1, FloatType::Float64), "0x1.999999999999ap-4");
        assert_eq!(
            literal(f64::from(0.1_f32), FloatType::Float32),
            "0x1.99999ap-4f"
        );
        assert_eq!(
            literal(f64::MAX, FloatType::Float64),
            "0x1.fffffffffffffp+1023"
        );
        assert_eq!(
            literal(5e-324, FloatType::Float64),
            "0x0.0000000000001p-1022"
        );
        assert_eq!(
            literal(f64::from(f32::from_bits(1)), FloatType::Float32),
            "0x1p-149f"
        );
    }

    #[test]
    fn strings_escape_every_byte_c_could_misread() {
        assert_eq!(string_literal("plain text"), "\"plain text\"");
        assert_eq!(
            string_literal("a \"quote\" and \\"),
            "\"a \\\"quote\\\" and \\\\\""
        );
        assert_eq!(string_literal("line\nbreak\ttab"), "\"line\\nbreak\\ttab\"");
        assert_eq!(string_literal("what??!"), "\"what\\?\\?!\"");
        assert_eq!(string_literal("é"), "\"\\303\\251\"");
        assert_eq!(string_literal("\u{0}1"), "\"\\0001\"");
    }

    #[test]
    fn integers_are_written_with_their_exact_type() {
        assert_eq!(integer_literal(5, IntegerType::Integer64), "INT64_C(5)");
        assert_eq!(integer_literal(-5, IntegerType::Integer64), "(-INT64_C(5))");
        assert_eq!(
            integer_literal(i128::from(i64::MIN), IntegerType::Integer64),
            "INT64_MIN"
        );
        assert_eq!(
            integer_literal(i128::from(u64::MAX), IntegerType::Unsigned64),
            "UINT64_C(18446744073709551615)"
        );
        assert_eq!(integer_literal(7, IntegerType::Unsigned64), "UINT64_C(7)");
        assert_eq!(
            integer_literal(-128, IntegerType::Integer8),
            "((int8_t)(-INT64_C(128)))"
        );
        assert_eq!(
            integer_literal(200, IntegerType::Unsigned8),
            "((uint8_t)INT64_C(200))"
        );
    }

    #[test]
    fn text_values_carry_their_length_in_bytes() {
        assert_eq!(
            value(&Value::Text("é!".to_owned())),
            "((lymra_string){\"\\303\\251!\", 3})"
        );
        assert_eq!(static_initializer(&Value::Text(String::new())), "{\"\", 0}");
    }
}
