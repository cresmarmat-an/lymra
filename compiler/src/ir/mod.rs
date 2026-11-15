/*!
 * The checked program: what the semantic checker hands to code generation.
 *
 * Every name is resolved to a numbered entity and every expression carries
 * its type, so no later stage looks anything up or checks anything again.
 * Spans are kept where a runtime error can happen, so it can point at the
 * source.
 */

mod types;

pub use types::{
    ArrayType, ChoiceType, CustomType, FieldType, FloatType, IntegerType, Layout, ListType,
    Sequence, Type, Value, ViewType,
};

use crate::ast::OptimizeMode;
use crate::source::Span;

/// Index into `Program::functions`.
pub type FunctionId = usize;
/// Index into `Program::constants`.
pub type ConstantId = usize;
/// Index into `Function::locals`.
pub type LocalId = usize;

#[derive(Clone, Debug)]
pub struct Program<'src> {
    pub constants: Vec<Constant<'src>>,
    pub functions: Vec<Function<'src>>,
    /// The entry point. A file without one can be checked but not built.
    pub main: Option<FunctionId>,
    /// The mode chosen with `#!optimize`, if the file sets one.
    pub optimize: Option<OptimizeMode>,
    /// Set by `#!prove bounds`: every index has to be proven in range while compiling.
    pub prove_bounds: bool,
}

/// A top-level constant, already computed.
#[derive(Clone, Debug)]
pub struct Constant<'src> {
    pub name: &'src str,
    pub value: Value,
}

#[derive(Clone, Debug)]
pub struct Function<'src> {
    pub name: &'src str,
    pub parameters: Vec<LocalId>,
    /// `None` for a function that returns nothing.
    pub return_type: Option<Type>,
    pub locals: Vec<Local<'src>>,
    pub body: Block,
}

/// A parameter, a binding, or a loop variable of one function.
#[derive(Clone, Debug)]
pub struct Local<'src> {
    pub name: &'src str,
    pub ty: Type,
    pub mutable: bool,
    /// True for a `variable` parameter, which reaches the caller's binding.
    pub by_reference: bool,
    /// The name where the local is declared.
    pub span: Span,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    pub statements: Vec<Statement>,
}

#[derive(Clone, Debug)]
pub enum Statement {
    Declare {
        local: LocalId,
        value: Expr,
    },
    /// `constant _ = value`: compute the value and drop it.
    Discard(Expr),
    /// The target's indexes are computed and checked, then the value, then the value is stored.
    Assign {
        target: Place,
        value: Expr,
    },
    /// A call to a function that returns nothing.
    Call(Call),
    /// A call to `print`: its parts in order, then a line break.
    Print(Vec<PrintPart>),
    If {
        branches: Vec<Branch>,
        otherwise: Option<Block>,
    },
    While {
        condition: Expr,
        body: Block,
    },
    /// `for variable in range(start, end)`. `variable` is `None` when it is named `_`.
    ForRange {
        variable: Option<LocalId>,
        start: Expr,
        end: Expr,
        body: Block,
    },
    /**
     * `for variable in items`, over an array or a list. The value is computed
     * once; each element is read when its turn comes, so a change the body
     * makes to a later element is seen. A list is walked over the elements
     * it holds when the loop starts, so the body may add to it.
     */
    ForEach {
        variable: Option<LocalId>,
        array: Expr,
        /// How many elements the value has, so how many times the body runs.
        length: Extent,
        body: Block,
    },
    /// `push(variable list, value)`: the value is computed, then added to the end of the list.
    Push {
        list: LocalId,
        value: Expr,
        /// The call, which a full data stack points at.
        span: Span,
    },
    /**
     * `match value`: the value is computed once, then the case for the
     * variant it holds runs, with the fields the case names bound to their
     * values. The cases handle every variant of the choice once.
     */
    Match {
        value: Expr,
        cases: Vec<Case>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Assert {
        condition: Expr,
    },
}

/// One `case` of a `match`.
#[derive(Clone, Debug)]
pub struct Case {
    /// The position of the variant in its choice.
    pub variant: usize,
    /// The locals the case declares, each holding one field of the variant.
    pub bindings: Vec<CaseBinding>,
    pub body: Block,
}

#[derive(Clone, Copy, Debug)]
pub struct CaseBinding {
    pub local: LocalId,
    /// The position of the field in its variant.
    pub field: usize,
}

/// Where an assignment stores its value: a local, or a part of it.
#[derive(Clone, Debug)]
pub struct Place {
    pub local: LocalId,
    /// The steps from the local inward: `grid[row].cells[column]` has three.
    pub steps: Vec<PlaceStep>,
    /// The place as written.
    pub span: Span,
}

/// One step into a value: an element of an array, or a field of a declared type.
#[derive(Clone, Debug)]
pub enum PlaceStep {
    Index(PlaceIndex),
    /// The position of the field in its type.
    Field(usize),
}

/// One `[index]` of a place.
#[derive(Clone, Debug)]
pub struct PlaceIndex {
    pub value: Expr,
    /// The type of the array, list or stack the index selects from.
    pub indexed: Type,
    /// Whether the index can fall outside its array and has to be checked when the program runs.
    pub checked: bool,
    /// The place up to and including this index, which a failed check points at.
    pub span: Span,
}

impl PlaceIndex {
    /// The length of the array, list or stack the index selects from.
    pub fn length(&self) -> Extent {
        match self.indexed.as_array() {
            Some(array) => Extent::Fixed(array.length),
            None => Extent::Dynamic,
        }
    }
}

/// How many elements a value that can be indexed has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extent {
    /// An array's length, part of its type.
    Fixed(u64),
    /// A list's length, which only the running program knows.
    Dynamic,
}

#[derive(Clone, Debug)]
pub struct Branch {
    pub condition: Expr,
    pub body: Block,
}

#[derive(Clone, Debug)]
pub enum PrintPart {
    Text(String),
    /// A value, and how it is written as text.
    Value {
        value: Expr,
        form: TextForm,
    },
}

/// How a value is written as text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextForm {
    Text,
    Boolean,
    Signed,
    Unsigned,
    /// The shortest decimal that reads back as the same float, written the same way on every system.
    Float(FloatType),
}

impl TextForm {
    /// How values of `ty` are written, or `None` for a value that holds others, which text cannot hold.
    pub fn of(ty: &Type) -> Option<TextForm> {
        match ty {
            Type::String => Some(Self::Text),
            Type::Boolean => Some(Self::Boolean),
            Type::Integer(integer) if integer.is_signed() => Some(Self::Signed),
            Type::Integer(_) => Some(Self::Unsigned),
            Type::Float(float) => Some(Self::Float(*float)),
            Type::Array(_) | Type::Custom(_) | Type::Choice(_) | Type::List(_) | Type::View(_) => {
                None
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Literal(Value),
    Local(LocalId),
    Constant(ConstantId),
    Negate {
        operand: Box<Expr>,
        /// Whether the operation can fail and has to be checked when the program runs.
        checked: bool,
    },
    Not(Box<Expr>),
    Arithmetic {
        operator: ArithmeticOperator,
        left: Box<Expr>,
        right: Box<Expr>,
        /**
         * Whether the operation can fail and has to be checked when the
         * program runs. The checker sets it on every operation; the
         * optimizer clears it where it proves the operation cannot overflow
         * or divide by zero.
         */
        checked: bool,
    },
    Compare {
        operator: ComparisonOperator,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Logic {
        operator: LogicOperator,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call(Call),
    /// `integer32(value)` and the like. The expression's type is the target type.
    Convert {
        operand: Box<Expr>,
        /// False when every value of the operand's type fits the target, so nothing can fail.
        checked: bool,
    },
    /// `wrapping_add(left, right)` and the like: arithmetic that wraps around its type and never fails.
    Wrapping {
        operator: WrappingOperator,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `items[index]`, an element of an array or a list.
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        /// The length of the array or list, which the index has to stay below.
        length: Extent,
        /// Whether the index can fall outside the array and has to be checked when the program runs.
        checked: bool,
    },
    /// `array(value)`: every element of the array is `value`. The expression's type is the array type.
    Fill(Box<Expr>),
    /// `list()`: a list with no elements yet. The expression's type is the list type.
    EmptyList,
    /// Every element of an array, a list or a view, seen through a view. Nothing is copied.
    View(Box<Expr>),
    /**
     * `slice(items, start, end)`: the elements of an array, a list or a view
     * from `start` up to `end`, as a view, or the bytes of a text between
     * them, as a text. The positions are computed in that order, then
     * checked when the program runs.
     */
    Slice {
        items: Box<Expr>,
        start: Box<Expr>,
        end: Box<Expr>,
    },
    /// `bytes(text)`: the bytes of a text as a `view[unsigned8]`.
    Bytes(Box<Expr>),
    /// `pop(variable stack)`: takes the top element off a stack and gives it. An empty stack stops the program.
    Pop(LocalId),
    /// `top(stack)`: the top element of a stack, left where it is. An empty stack stops the program.
    Top(Box<Expr>),
    /// `length(value)` of a text, or of an array that has to be computed because computing it has effects.
    Length(Box<Expr>),
    /// `Frog(name = "Kero", age = 3)`: a value of a declared type, its fields in the order they are written.
    Construct(Vec<FieldValue>),
    /// `value.field`, by the field's position in its type.
    Field {
        object: Box<Expr>,
        field: usize,
    },
    /// `Circle(radius = 2)` or `Dot`: a value of a choice, the variant at `variant` with its fields in the order written.
    Variant {
        variant: usize,
        fields: Vec<FieldValue>,
    },
}

/// The value given for one field in a construction.
#[derive(Clone, Debug)]
pub struct FieldValue {
    /// The position of the field in its type.
    pub field: usize,
    pub value: Expr,
}

impl Expr {
    /// Whether the expression reads `local` or passes it to a call.
    pub fn mentions(&self, local: LocalId) -> bool {
        match &self.kind {
            ExprKind::Local(id) => *id == local,
            ExprKind::Literal(_) | ExprKind::Constant(_) | ExprKind::EmptyList => false,
            ExprKind::Negate { operand, .. }
            | ExprKind::Not(operand)
            | ExprKind::Convert { operand, .. }
            | ExprKind::Fill(operand)
            | ExprKind::Length(operand)
            | ExprKind::View(operand)
            | ExprKind::Bytes(operand)
            | ExprKind::Top(operand)
            | ExprKind::Field {
                object: operand, ..
            } => operand.mentions(local),
            ExprKind::Pop(stack) => *stack == local,
            ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => {
                fields.iter().any(|field| field.value.mentions(local))
            }
            ExprKind::Slice { items, start, end } => {
                items.mentions(local) || start.mentions(local) || end.mentions(local)
            }
            ExprKind::Arithmetic { left, right, .. }
            | ExprKind::Compare { left, right, .. }
            | ExprKind::Logic { left, right, .. }
            | ExprKind::Wrapping { left, right, .. }
            | ExprKind::Index {
                array: left,
                index: right,
                ..
            } => left.mentions(local) || right.mentions(local),
            ExprKind::Call(call) => call.arguments.iter().any(|argument| match argument {
                Argument::Reference { local: id, .. } => *id == local,
                Argument::Value(value) => value.mentions(local),
            }),
        }
    }

    /**
     * Whether computing the expression can be seen from outside it: a call
     * can print or stop the program, and so can a checked operation. Reading
     * values cannot.
     */
    pub fn has_effects(&self) -> bool {
        match &self.kind {
            ExprKind::Literal(_)
            | ExprKind::Local(_)
            | ExprKind::Constant(_)
            | ExprKind::EmptyList => false,
            ExprKind::Call(_) => true,
            ExprKind::Not(operand)
            | ExprKind::Fill(operand)
            | ExprKind::Length(operand)
            | ExprKind::View(operand)
            | ExprKind::Bytes(operand)
            | ExprKind::Field {
                object: operand, ..
            } => operand.has_effects(),
            // A slice whose positions fall outside its items stops the program, and so does an empty stack.
            ExprKind::Slice { .. } | ExprKind::Pop(_) | ExprKind::Top(_) => true,
            ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => {
                fields.iter().any(|field| field.value.has_effects())
            }
            ExprKind::Negate { operand, checked } | ExprKind::Convert { operand, checked } => {
                *checked || operand.has_effects()
            }
            ExprKind::Arithmetic {
                left,
                right,
                checked,
                ..
            } => *checked || left.has_effects() || right.has_effects(),
            ExprKind::Compare { left, right, .. }
            | ExprKind::Logic { left, right, .. }
            | ExprKind::Wrapping { left, right, .. } => left.has_effects() || right.has_effects(),
            ExprKind::Index {
                array,
                index,
                checked,
                ..
            } => *checked || array.has_effects() || index.has_effects(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Call {
    pub function: FunctionId,
    pub arguments: Vec<Argument>,
}

#[derive(Clone, Debug)]
pub enum Argument {
    Value(Expr),
    /// A binding passed with `variable`, which the called function may change.
    Reference {
        local: LocalId,
        /// The argument as written, `variable` included.
        span: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArithmeticOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

impl ArithmeticOperator {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Remainder => "%",
        }
    }
}

/// The operations that have a wrapping form. Division cannot overflow but by -1, so it has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrappingOperator {
    Add,
    Subtract,
    Multiply,
}

impl WrappingOperator {
    /// The built-in function that performs it.
    pub const fn function(self) -> &'static str {
        match self {
            Self::Add => "wrapping_add",
            Self::Subtract => "wrapping_subtract",
            Self::Multiply => "wrapping_multiply",
        }
    }

    pub const fn from_function(name: &str) -> Option<Self> {
        match name.as_bytes() {
            b"wrapping_add" => Some(Self::Add),
            b"wrapping_subtract" => Some(Self::Subtract),
            b"wrapping_multiply" => Some(Self::Multiply),
            _ => None,
        }
    }

    /// The same operation, without the wrapping.
    pub const fn arithmetic(self) -> ArithmeticOperator {
        match self {
            Self::Add => ArithmeticOperator::Add,
            Self::Subtract => ArithmeticOperator::Subtract,
            Self::Multiply => ArithmeticOperator::Multiply,
        }
    }

    /// The result for two values of `ty`: exact, then wrapped around the type.
    pub const fn apply(self, left: i128, right: i128, ty: IntegerType) -> i128 {
        // Only the low 64 bits of each operand decide the low bits of the result.
        let (left, right) = (left as u64, right as u64);
        let bits = match self {
            Self::Add => left.wrapping_add(right),
            Self::Subtract => left.wrapping_sub(right),
            Self::Multiply => left.wrapping_mul(right),
        };
        ty.from_bits(bits)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

impl ComparisonOperator {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
        }
    }

    /// True for `==` and `!=`, which every type supports. The others need numbers.
    pub const fn is_equality(self) -> bool {
        matches!(self, Self::Equal | Self::NotEqual)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicOperator {
    And,
    Or,
}
