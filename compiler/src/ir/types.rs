//! The types and compile-time values of edition 0.1.

use std::fmt;
use std::sync::Arc;

/// The built-in integer types. Each states its size in bits (spec 4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntegerType {
    Integer8,
    Integer16,
    Integer32,
    Integer64,
    Unsigned8,
    Unsigned16,
    Unsigned32,
    Unsigned64,
}

impl IntegerType {
    pub const ALL: [IntegerType; 8] = [
        Self::Integer8,
        Self::Integer16,
        Self::Integer32,
        Self::Integer64,
        Self::Unsigned8,
        Self::Unsigned16,
        Self::Unsigned32,
        Self::Unsigned64,
    ];

    /// The type an integer literal takes when nothing asks for another one.
    pub const DEFAULT: IntegerType = IntegerType::Integer64;

    pub const fn name(self) -> &'static str {
        match self {
            Self::Integer8 => "integer8",
            Self::Integer16 => "integer16",
            Self::Integer32 => "integer32",
            Self::Integer64 => "integer64",
            Self::Unsigned8 => "unsigned8",
            Self::Unsigned16 => "unsigned16",
            Self::Unsigned32 => "unsigned32",
            Self::Unsigned64 => "unsigned64",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|ty| ty.name() == name)
    }

    pub const fn is_signed(self) -> bool {
        matches!(
            self,
            Self::Integer8 | Self::Integer16 | Self::Integer32 | Self::Integer64
        )
    }

    pub const fn bits(self) -> u32 {
        match self {
            Self::Integer8 | Self::Unsigned8 => 8,
            Self::Integer16 | Self::Unsigned16 => 16,
            Self::Integer32 | Self::Unsigned32 => 32,
            Self::Integer64 | Self::Unsigned64 => 64,
        }
    }

    pub const fn minimum(self) -> i128 {
        if self.is_signed() {
            -(1_i128 << (self.bits() - 1))
        } else {
            0
        }
    }

    pub const fn maximum(self) -> i128 {
        if self.is_signed() {
            (1_i128 << (self.bits() - 1)) - 1
        } else {
            (1_i128 << self.bits()) - 1
        }
    }

    pub const fn contains(self, value: i128) -> bool {
        self.minimum() <= value && value <= self.maximum()
    }
}

/// A type the checker knows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Integer(IntegerType),
    Boolean,
    String,
    /// `array[element, length]`, shared so that copying a type stays cheap.
    Array(Arc<ArrayType>),
}

/// A fixed number of elements of one type, known while compiling (spec 4.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArrayType {
    pub element: Type,
    /// At least one.
    pub length: u64,
}

impl Type {
    pub fn array(element: Type, length: u64) -> Type {
        Type::Array(Arc::new(ArrayType { element, length }))
    }

    pub const fn as_integer(&self) -> Option<IntegerType> {
        match self {
            Self::Integer(integer) => Some(*integer),
            Self::Boolean | Self::String | Self::Array(_) => None,
        }
    }

    pub fn as_array(&self) -> Option<&ArrayType> {
        match self {
            Self::Array(array) => Some(array),
            Self::Integer(_) | Self::Boolean | Self::String => None,
        }
    }

    /**
     * The size of a value in bytes, or `None` when it does not fit a `u64`.
     * Sizes are those of a 64-bit system on every system, so a program that
     * one compiler accepts, every compiler accepts.
     */
    pub fn size(&self) -> Option<u64> {
        match self {
            Self::Integer(integer) => Some(u64::from(integer.bits() / 8)),
            Self::Boolean => Some(1),
            // A text value is a pointer and a length.
            Self::String => Some(16),
            Self::Array(array) => array.element.size()?.checked_mul(array.length),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(integer) => formatter.write_str(integer.name()),
            Self::Boolean => formatter.write_str("boolean"),
            Self::String => formatter.write_str("string"),
            Self::Array(array) => write!(formatter, "array[{}, {}]", array.element, array.length),
        }
    }
}

/// A value known at compile time: a literal, a top-level constant, or a folded expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// The value always lies within the range of `ty`.
    Integer {
        value: i128,
        ty: IntegerType,
    },
    Boolean(bool),
    Text(String),
}

impl Value {
    pub fn ty(&self) -> Type {
        match self {
            Self::Integer { ty, .. } => Type::Integer(*ty),
            Self::Boolean(_) => Type::Boolean,
            Self::Text(_) => Type::String,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_match_the_bit_widths() {
        assert_eq!(IntegerType::Integer8.minimum(), -128);
        assert_eq!(IntegerType::Integer8.maximum(), 127);
        assert_eq!(IntegerType::Unsigned8.maximum(), 255);
        assert_eq!(IntegerType::Integer64.minimum(), i128::from(i64::MIN));
        assert_eq!(IntegerType::Unsigned64.maximum(), i128::from(u64::MAX));
        assert!(IntegerType::Unsigned16.contains(65_535));
        assert!(!IntegerType::Unsigned16.contains(-1));
    }

    #[test]
    fn arrays_are_named_and_sized_as_written() {
        let row = Type::array(Type::Integer(IntegerType::Integer32), 16);
        let board = Type::array(row.clone(), 8);
        assert_eq!(board.to_string(), "array[array[integer32, 16], 8]");
        assert_eq!(board.size(), Some(512));
        assert_eq!(Type::array(board.clone(), u64::MAX).size(), None);
        assert_eq!(board.as_array().map(|array| &array.element), Some(&row));
        assert_ne!(row, Type::array(Type::Integer(IntegerType::Integer32), 15));
    }

    #[test]
    fn names_round_trip() {
        for ty in IntegerType::ALL {
            assert_eq!(IntegerType::from_name(ty.name()), Some(ty));
        }
        assert_eq!(IntegerType::from_name("i64"), None);
    }
}
