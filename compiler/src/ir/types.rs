//! The types and compile-time values of edition 0.1, phase 1.

use std::fmt;

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

/// A type the checker knows. Values of every type can be printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Integer(IntegerType),
    Boolean,
    String,
}

impl Type {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Integer(integer) => integer.name(),
            Self::Boolean => "boolean",
            Self::String => "string",
        }
    }

    pub const fn as_integer(self) -> Option<IntegerType> {
        match self {
            Self::Integer(integer) => Some(integer),
            Self::Boolean | Self::String => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
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
    fn names_round_trip() {
        for ty in IntegerType::ALL {
            assert_eq!(IntegerType::from_name(ty.name()), Some(ty));
        }
        assert_eq!(IntegerType::from_name("i64"), None);
    }
}
