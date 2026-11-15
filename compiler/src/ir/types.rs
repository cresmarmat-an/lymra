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

    /**
     * The value of this type that `bits` stand for: the low bits of the
     * type's size, read as two's complement for a signed type. This is how
     * wrapping arithmetic brings a result back into the type.
     */
    pub const fn from_bits(self, bits: u64) -> i128 {
        let width = self.bits();
        let low = if width == 64 {
            bits
        } else {
            bits & ((1 << width) - 1)
        };
        if self.is_signed() && (low >> (width - 1)) & 1 == 1 {
            low as i128 - (1 << width)
        } else {
            low as i128
        }
    }
}

/// The built-in float types: IEEE 754 binary floats of 32 and 64 bits (spec 4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FloatType {
    Float32,
    Float64,
}

impl FloatType {
    /// The type a float literal takes when nothing asks for another one.
    pub const DEFAULT: FloatType = FloatType::Float64;

    pub const fn name(self) -> &'static str {
        match self {
            Self::Float32 => "float32",
            Self::Float64 => "float64",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Float32, Self::Float64]
            .into_iter()
            .find(|ty| ty.name() == name)
    }

    /// `value` as this type holds it: rounded to the nearest `float32`, or unchanged.
    pub fn round(self, value: f64) -> f64 {
        match self {
            Self::Float32 => f64::from(value as f32),
            Self::Float64 => value,
        }
    }
}

/// A type the checker knows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Integer(IntegerType),
    Float(FloatType),
    Boolean,
    String,
    /// `array[element, length]`, shared so that copying a type stays cheap.
    Array(Arc<ArrayType>),
    /// A type the program declares with `type`, shared like an array.
    Custom(Arc<CustomType>),
    /// A type the program declares with `choice`, shared like an array.
    Choice(Arc<ChoiceType>),
    /// `list[element]` or `stack[element]`, which grow while they live (spec 4.4), shared like an array.
    List(Arc<ListType>),
    /// `view[element]`, a read-only window into elements kept elsewhere (spec 4.4), shared like an array.
    View(Arc<ViewType>),
}

/**
 * Named fields stored one after another: a `type` declaration (spec 4.6),
 * or one variant of a `choice`.
 */
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CustomType {
    /// A type's name is unique among the program's types, so it identifies the type.
    pub name: String,
    /// In the order the declaration lists them. A type has at least one; a variant can have none.
    pub fields: Vec<FieldType>,
}

/// A `choice` declaration: a value that is exactly one of its variants (spec 4.7).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChoiceType {
    /// Unique among the program's types, so it identifies the type.
    pub name: String,
    /// At least one, in the order the declaration lists them.
    pub variants: Vec<CustomType>,
}

impl ChoiceType {
    /// The position and the fields of the variant called `name`.
    pub fn variant(&self, name: &str) -> Option<(usize, &CustomType)> {
        self.variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.name == name)
    }

    /// The type of the tag that says which variant a value holds: the smallest one with room for every variant.
    pub fn tag_type(&self) -> IntegerType {
        let largest = self.variants.len().saturating_sub(1);
        [IntegerType::Unsigned8, IntegerType::Unsigned16]
            .into_iter()
            .find(|tag| tag.contains(largest as i128))
            .unwrap_or(IntegerType::Unsigned32)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldType {
    pub name: String,
    pub ty: Type,
}

impl CustomType {
    /// The position and the type of the field called `name`.
    pub fn field(&self, name: &str) -> Option<(usize, &FieldType)> {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == name)
    }

    /// The fields laid out as a C struct lays them out, each at a multiple of its alignment.
    fn layout(&self) -> Option<Layout> {
        let mut end = 0_u64;
        let mut align = 1;
        for field in &self.fields {
            let layout = field.ty.layout()?;
            end = end
                .checked_next_multiple_of(layout.align)?
                .checked_add(layout.size)?;
            align = align.max(layout.align);
        }
        Some(Layout {
            size: end.checked_next_multiple_of(align)?,
            align,
        })
    }
}

/// How many bytes a value takes, and the multiple of bytes its address is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: u64,
    pub align: u64,
}

/// A fixed number of elements of one type, known while compiling (spec 4.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArrayType {
    pub element: Type,
    /// At least one.
    pub length: u64,
}

/**
 * Elements of one type that a program adds one at a time, in place, on a
 * data stack (spec 7.2). A stack can also take its top element off.
 */
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ListType {
    pub element: Type,
    pub kind: Sequence,
}

/// Whether elements only come onto a list, or can go again from the top, as on a stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sequence {
    /// `list[T]`: grows only.
    List,
    /// `stack[T]`: grows with `push` and shrinks with `pop`.
    Stack,
}

impl Sequence {
    /// The name of the type, as written: `list` or `stack`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Stack => "stack",
        }
    }
}

/// Elements of one type that an array, a list or a text keeps, read through a window that owns none of them (spec 4.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ViewType {
    pub element: Type,
}

impl Type {
    pub fn array(element: Type, length: u64) -> Type {
        Type::Array(Arc::new(ArrayType { element, length }))
    }

    pub fn list(element: Type) -> Type {
        Type::sequence(element, Sequence::List)
    }

    /// A list or a stack of `element`, as `kind` says.
    pub fn sequence(element: Type, kind: Sequence) -> Type {
        Type::List(Arc::new(ListType { element, kind }))
    }

    pub fn view(element: Type) -> Type {
        Type::View(Arc::new(ViewType { element }))
    }

    pub const fn as_integer(&self) -> Option<IntegerType> {
        match self {
            Self::Integer(integer) => Some(*integer),
            Self::Float(_)
            | Self::Boolean
            | Self::String
            | Self::Array(_)
            | Self::Custom(_)
            | Self::Choice(_)
            | Self::List(_)
            | Self::View(_) => None,
        }
    }

    pub const fn as_float(&self) -> Option<FloatType> {
        match self {
            Self::Float(float) => Some(*float),
            Self::Integer(_)
            | Self::Boolean
            | Self::String
            | Self::Array(_)
            | Self::Custom(_)
            | Self::Choice(_)
            | Self::List(_)
            | Self::View(_) => None,
        }
    }

    pub fn as_array(&self) -> Option<&ArrayType> {
        match self {
            Self::Array(array) => Some(array),
            Self::Integer(_)
            | Self::Float(_)
            | Self::Boolean
            | Self::String
            | Self::Custom(_)
            | Self::Choice(_)
            | Self::List(_)
            | Self::View(_) => None,
        }
    }

    pub fn as_list(&self) -> Option<&ListType> {
        match self {
            Self::List(list) => Some(list),
            Self::Integer(_)
            | Self::Float(_)
            | Self::Boolean
            | Self::String
            | Self::Array(_)
            | Self::Custom(_)
            | Self::Choice(_)
            | Self::View(_) => None,
        }
    }

    pub fn as_view(&self) -> Option<&ViewType> {
        match self {
            Self::View(view) => Some(view),
            Self::Integer(_)
            | Self::Float(_)
            | Self::Boolean
            | Self::String
            | Self::Array(_)
            | Self::Custom(_)
            | Self::Choice(_)
            | Self::List(_) => None,
        }
    }

    /// Whether values of the type hold other values: arrays, lists, and types declared with `type` or `choice`.
    pub const fn is_compound(&self) -> bool {
        matches!(
            self,
            Self::Array(_) | Self::Custom(_) | Self::Choice(_) | Self::List(_)
        )
    }

    /// Whether a value of the type holds something that grows while it lives, so it needs a data stack (spec 4.3).
    pub fn is_growable(&self) -> bool {
        match self {
            Self::List(_) => true,
            Self::Array(array) => array.element.is_growable(),
            Self::Custom(custom) => custom.fields.iter().any(|field| field.ty.is_growable()),
            Self::Choice(choice) => choice
                .variants
                .iter()
                .any(|variant| variant.fields.iter().any(|field| field.ty.is_growable())),
            Self::Integer(_) | Self::Float(_) | Self::Boolean | Self::String | Self::View(_) => {
                false
            }
        }
    }

    /// Whether a value of the type holds a view, which shows data that belongs to something else.
    pub fn holds_view(&self) -> bool {
        match self {
            Self::View(_) => true,
            Self::Array(array) => array.element.holds_view(),
            Self::List(list) => list.element.holds_view(),
            Self::Custom(custom) => custom.fields.iter().any(|field| field.ty.holds_view()),
            Self::Choice(choice) => choice
                .variants
                .iter()
                .any(|variant| variant.fields.iter().any(|field| field.ty.holds_view())),
            Self::Integer(_) | Self::Float(_) | Self::Boolean | Self::String => false,
        }
    }

    /// The type of the elements, for a list or a view, whose length only the running program knows.
    pub fn dynamic_elements(&self) -> Option<&Type> {
        match self {
            Self::List(list) => Some(&list.element),
            Self::View(view) => Some(&view.element),
            Self::Integer(_)
            | Self::Float(_)
            | Self::Boolean
            | Self::String
            | Self::Array(_)
            | Self::Custom(_)
            | Self::Choice(_) => None,
        }
    }

    /**
     * How a value is laid out, as on a 64-bit system, where every field sits
     * at a multiple of its alignment; `None` when the size does not fit a
     * `u64`. Other systems take as much or less, so a program that one
     * compiler accepts, every compiler accepts.
     */
    pub fn layout(&self) -> Option<Layout> {
        match self {
            Self::Integer(integer) => {
                let size = u64::from(integer.bits() / 8);
                Some(Layout { size, align: size })
            }
            Self::Float(FloatType::Float32) => Some(Layout { size: 4, align: 4 }),
            Self::Float(FloatType::Float64) => Some(Layout { size: 8, align: 8 }),
            Self::Boolean => Some(Layout { size: 1, align: 1 }),
            // A text value is a pointer and a length.
            Self::String => Some(Layout { size: 16, align: 8 }),
            // Where its elements start, how many there are, and the data stack they grow on.
            Self::List(_) => Some(Layout { size: 24, align: 8 }),
            // Where its elements start, and how many there are.
            Self::View(_) => Some(Layout { size: 16, align: 8 }),
            Self::Array(array) => {
                let element = array.element.layout()?;
                Some(Layout {
                    size: element.size.checked_mul(array.length)?,
                    align: element.align,
                })
            }
            Self::Custom(custom) => custom.layout(),
            // The tag, then the fields of whichever variant needs the most room.
            Self::Choice(choice) => {
                let tag = u64::from(choice.tag_type().bits() / 8);
                let mut largest = 0_u64;
                let mut payload_align = 1;
                for variant in &choice.variants {
                    let layout = variant.layout()?;
                    largest = largest.max(layout.size);
                    payload_align = payload_align.max(layout.align);
                }
                let align = payload_align.max(tag);
                let size = tag
                    .checked_next_multiple_of(payload_align)?
                    .checked_add(largest.checked_next_multiple_of(payload_align)?)?
                    .checked_next_multiple_of(align)?;
                Some(Layout { size, align })
            }
        }
    }

    /// The size of a value in bytes. See `layout`.
    pub fn size(&self) -> Option<u64> {
        self.layout().map(|layout| layout.size)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(integer) => formatter.write_str(integer.name()),
            Self::Float(float) => formatter.write_str(float.name()),
            Self::Boolean => formatter.write_str("boolean"),
            Self::String => formatter.write_str("string"),
            Self::Array(array) => write!(formatter, "array[{}, {}]", array.element, array.length),
            Self::Custom(custom) => formatter.write_str(&custom.name),
            Self::Choice(choice) => formatter.write_str(&choice.name),
            Self::List(list) => write!(formatter, "{}[{}]", list.kind.name(), list.element),
            Self::View(view) => write!(formatter, "view[{}]", view.element),
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
    /// The bits of the value as a `f64`, which holds every `float32` exactly too.
    Float {
        bits: u64,
        ty: FloatType,
    },
    Boolean(bool),
    Text(String),
}

impl Value {
    /// A float of type `ty`, rounded to it.
    pub fn float(value: f64, ty: FloatType) -> Value {
        Value::Float {
            bits: ty.round(value).to_bits(),
            ty,
        }
    }

    pub fn ty(&self) -> Type {
        match self {
            Self::Integer { ty, .. } => Type::Integer(*ty),
            Self::Float { ty, .. } => Type::Float(*ty),
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
    fn bits_wrap_into_the_type() {
        assert_eq!(IntegerType::Integer8.from_bits(0x80), -128);
        assert_eq!(IntegerType::Integer8.from_bits(0x17F), 127);
        assert_eq!(IntegerType::Unsigned8.from_bits(0x1FF), 255);
        assert_eq!(IntegerType::Integer64.from_bits(u64::MAX), -1);
        assert_eq!(
            IntegerType::Unsigned64.from_bits(u64::MAX),
            i128::from(u64::MAX)
        );
        assert_eq!(IntegerType::Integer32.from_bits(1 << 31), -(1 << 31));
    }

    #[test]
    fn declared_types_are_laid_out_like_c_structs() {
        let field = |name: &str, ty: Type| FieldType {
            name: name.to_owned(),
            ty,
        };
        let small = Type::Custom(Arc::new(CustomType {
            name: "Small".to_owned(),
            fields: vec![
                field("flag", Type::Boolean),
                field("count", Type::Integer(IntegerType::Integer64)),
                field("mark", Type::Integer(IntegerType::Unsigned8)),
            ],
        }));
        assert_eq!(small.layout(), Some(Layout { size: 24, align: 8 }));
        assert_eq!(small.to_string(), "Small");
        assert_eq!(Type::array(small, 3).size(), Some(72));
    }

    #[test]
    fn choices_hold_a_tag_and_their_largest_variant() {
        let variant = |name: &str, fields: Vec<Type>| CustomType {
            name: name.to_owned(),
            fields: fields
                .into_iter()
                .enumerate()
                .map(|(position, ty)| FieldType {
                    name: format!("field_{position}"),
                    ty,
                })
                .collect(),
        };
        let choice = |variants: Vec<CustomType>| {
            Type::Choice(Arc::new(ChoiceType {
                name: "Shape".to_owned(),
                variants,
            }))
        };
        let wide = Type::Integer(IntegerType::Integer64);
        let byte = Type::Integer(IntegerType::Unsigned8);
        let shape = choice(vec![
            variant("Circle", vec![wide.clone()]),
            variant("Rect", vec![wide.clone(), wide.clone()]),
            variant("Dot", vec![]),
        ]);
        assert_eq!(shape.layout(), Some(Layout { size: 24, align: 8 }));
        let flags = choice(vec![variant("On", vec![]), variant("Off", vec![])]);
        assert_eq!(flags.layout(), Some(Layout { size: 1, align: 1 }));
        // Three bytes of fields round up to the two-byte alignment of the other variant.
        let odd = choice(vec![
            variant("Bytes", vec![byte.clone(), byte.clone(), byte]),
            variant("Short", vec![Type::Integer(IntegerType::Integer16)]),
        ]);
        assert_eq!(odd.layout(), Some(Layout { size: 6, align: 2 }));
        let many = ChoiceType {
            name: "Many".to_owned(),
            variants: (0..300)
                .map(|index| variant(&format!("V{index}"), vec![]))
                .collect(),
        };
        assert_eq!(many.tag_type(), IntegerType::Unsigned16);
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
    fn lists_grow_and_are_named_as_written() {
        let numbers = Type::list(Type::Integer(IntegerType::Integer32));
        assert_eq!(numbers.to_string(), "list[integer32]");
        assert!(numbers.is_growable());
        assert!(numbers.is_compound());
        assert!(Type::array(numbers.clone(), 2).is_growable());
        assert!(!Type::array(Type::Boolean, 2).is_growable());
        assert_eq!(
            numbers.as_list().map(|list| &list.element),
            Some(&Type::Integer(IntegerType::Integer32))
        );
    }

    #[test]
    fn names_round_trip() {
        for ty in IntegerType::ALL {
            assert_eq!(IntegerType::from_name(ty.name()), Some(ty));
        }
        assert_eq!(IntegerType::from_name("i64"), None);
    }
}
