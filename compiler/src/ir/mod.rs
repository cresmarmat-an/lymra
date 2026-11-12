/*!
 * The checked program: what the semantic checker hands to code generation.
 *
 * Every name is resolved to a numbered entity and every expression carries
 * its type, so no later stage looks anything up or checks anything again.
 * Spans are kept where a runtime error can happen, so it can point at the
 * source.
 */

mod types;

pub use types::{ArrayType, IntegerType, Type, Value};

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
     * `for variable in array`. The array is computed once; each element is
     * read when its turn comes, so a change the body makes to a later
     * element is seen.
     */
    ForEach {
        variable: Option<LocalId>,
        array: Expr,
        /// How many elements the array has, so how many times the body runs.
        length: u64,
        body: Block,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Assert {
        condition: Expr,
    },
}

/// Where an assignment stores its value: a local, or an element inside it.
#[derive(Clone, Debug)]
pub struct Place {
    pub local: LocalId,
    /// The indexes from the local inward: `grid[row][column]` has two.
    pub indexes: Vec<PlaceIndex>,
}

/// One `[index]` of a place.
#[derive(Clone, Debug)]
pub struct PlaceIndex {
    pub value: Expr,
    /// The length of the array the index selects from.
    pub length: u64,
    /// Whether the index can fall outside its array and has to be checked when the program runs.
    pub checked: bool,
    /// The place up to and including this index, which a failed check points at.
    pub span: Span,
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
}

impl TextForm {
    /// How values of `ty` are written, or `None` for an array, which text cannot hold.
    pub fn of(ty: &Type) -> Option<TextForm> {
        match ty {
            Type::String => Some(Self::Text),
            Type::Boolean => Some(Self::Boolean),
            Type::Integer(integer) if integer.is_signed() => Some(Self::Signed),
            Type::Integer(_) => Some(Self::Unsigned),
            Type::Array(_) => None,
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
    /// `array[index]`, an element of an array.
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        /// The length of the array, which the index has to stay below.
        length: u64,
        /// Whether the index can fall outside the array and has to be checked when the program runs.
        checked: bool,
    },
    /// `array(value)`: every element of the array is `value`. The expression's type is the array type.
    Fill(Box<Expr>),
    /// `length(value)` of a text, or of an array that has to be computed because computing it has effects.
    Length(Box<Expr>),
}

impl Expr {
    /// Whether the expression reads `local` or passes it to a call.
    pub fn mentions(&self, local: LocalId) -> bool {
        match &self.kind {
            ExprKind::Local(id) => *id == local,
            ExprKind::Literal(_) | ExprKind::Constant(_) => false,
            ExprKind::Negate { operand, .. }
            | ExprKind::Not(operand)
            | ExprKind::Convert { operand, .. }
            | ExprKind::Fill(operand)
            | ExprKind::Length(operand) => operand.mentions(local),
            ExprKind::Arithmetic { left, right, .. }
            | ExprKind::Compare { left, right, .. }
            | ExprKind::Logic { left, right, .. }
            | ExprKind::Index {
                array: left,
                index: right,
                ..
            } => left.mentions(local) || right.mentions(local),
            ExprKind::Call(call) => call.arguments.iter().any(|argument| match argument {
                Argument::Reference(id) => *id == local,
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
            ExprKind::Literal(_) | ExprKind::Local(_) | ExprKind::Constant(_) => false,
            ExprKind::Call(_) => true,
            ExprKind::Not(operand) | ExprKind::Fill(operand) | ExprKind::Length(operand) => {
                operand.has_effects()
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
            ExprKind::Compare { left, right, .. } | ExprKind::Logic { left, right, .. } => {
                left.has_effects() || right.has_effects()
            }
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
    Reference(LocalId),
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
