/*!
 * The checked program: what the semantic checker hands to code generation.
 *
 * Every name is resolved to a numbered entity and every expression carries
 * its type, so no later stage looks anything up or checks anything again.
 * Spans are kept where a runtime error can happen, so it can point at the
 * source.
 */

mod types;

pub use types::{IntegerType, Type, Value};

use crate::ast::OptimizeMode;
use crate::source::Span;

/// Index into `Program::functions`.
pub type FunctionId = usize;
/// Index into `Program::constants`.
pub type ConstantId = usize;
/// Index into `Function::locals`.
pub type LocalId = usize;

#[derive(Debug)]
pub struct Program<'src> {
    pub constants: Vec<Constant<'src>>,
    pub functions: Vec<Function<'src>>,
    /// The entry point. A file without one can be checked but not built.
    pub main: Option<FunctionId>,
    /// The mode chosen with `#!optimize`, if the file sets one.
    pub optimize: Option<OptimizeMode>,
}

/// A top-level constant, already computed.
#[derive(Debug)]
pub struct Constant<'src> {
    pub name: &'src str,
    pub value: Value,
}

#[derive(Debug)]
pub struct Function<'src> {
    pub name: &'src str,
    pub parameters: Vec<LocalId>,
    /// `None` for a function that returns nothing.
    pub return_type: Option<Type>,
    pub locals: Vec<Local<'src>>,
    pub body: Block,
}

/// A parameter, a binding, or a loop variable of one function.
#[derive(Debug)]
pub struct Local<'src> {
    pub name: &'src str,
    pub ty: Type,
    pub mutable: bool,
    /// True for a `variable` parameter, which reaches the caller's binding.
    pub by_reference: bool,
}

#[derive(Debug, Default)]
pub struct Block {
    pub statements: Vec<Statement>,
}

#[derive(Debug)]
pub enum Statement {
    Declare {
        local: LocalId,
        value: Expr,
    },
    /// `constant _ = value`: compute the value and drop it.
    Discard(Expr),
    Assign {
        target: LocalId,
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
    Return(Option<Expr>),
    Break,
    Continue,
    Assert {
        condition: Expr,
    },
}

#[derive(Debug)]
pub struct Branch {
    pub condition: Expr,
    pub body: Block,
}

#[derive(Debug)]
pub enum PrintPart {
    Text(String),
    Value(Expr),
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug)]
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
    /// `to_integer32(value)` and the like. The expression's type is the target type.
    Convert {
        operand: Box<Expr>,
        /// False when every value of the operand's type fits the target, so nothing can fail.
        checked: bool,
    },
}

#[derive(Debug)]
pub struct Call {
    pub function: FunctionId,
    pub arguments: Vec<Argument>,
}

#[derive(Debug)]
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
