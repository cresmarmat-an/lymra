//! How Lymra types, names and values are written in C.

use crate::ir::{IntegerType, Type, Value};

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

pub(super) fn type_name(ty: Type) -> &'static str {
    match ty {
        Type::Integer(integer) => integer_type(integer),
        Type::Boolean => "bool",
        Type::String => "lymra_string",
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

/// A value inside a function body.
pub(super) fn value(value: &Value) -> String {
    match value {
        Value::Integer { value, ty } => integer_literal(*value, *ty),
        Value::Boolean(true) => "true".to_owned(),
        Value::Boolean(false) => "false".to_owned(),
        Value::Text(text) => text_value(text),
    }
}

/// A value as the initializer of a static constant, where C allows no compound literals.
pub(super) fn static_initializer(value: &Value) -> String {
    match value {
        Value::Text(text) => string_fields(text),
        Value::Integer { .. } | Value::Boolean(_) => self::value(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
