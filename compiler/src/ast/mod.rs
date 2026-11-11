/*!
 * The syntax tree: a Lymra file as structured data.
 *
 * The tree borrows names from the source text, so building it allocates only
 * for the nodes themselves. It holds what was written and nothing more. No
 * names are resolved and no types are checked here.
 */

mod printer;
pub mod visit;

pub use printer::print_module;

use crate::source::Span;

/// One source file.
#[derive(Debug)]
pub struct Module<'src> {
    pub directives: Vec<Directive>,
    pub items: Vec<Item<'src>>,
}

/// An identifier where it is declared or used, with its place in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name<'src> {
    pub text: &'src str,
    pub span: Span,
}

/// Names joined by dots, as in `math.geometry`.
#[derive(Debug)]
pub struct Path<'src> {
    pub segments: Vec<Name<'src>>,
    pub span: Span,
}

/// A `#!` line: an instruction to the compiler about the whole file.
#[derive(Debug)]
pub struct Directive {
    pub kind: DirectiveKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectiveKind {
    Edition(Edition),
    Optimize(OptimizeMode),
    ProveBounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edition {
    V0_1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptimizeMode {
    Speed,
    Size,
    Debug,
}

/// A top-level declaration.
#[derive(Debug)]
pub struct Item<'src> {
    pub visibility: Visibility,
    pub kind: ItemKind<'src>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Debug)]
pub enum ItemKind<'src> {
    Use(Path<'src>),
    Constant(Binding<'src>),
    Function(Function<'src>),
    Type(TypeDeclaration<'src>),
    Choice(ChoiceDeclaration<'src>),
}

#[derive(Debug)]
pub struct Function<'src> {
    pub name: Name<'src>,
    pub parameters: Vec<Parameter<'src>>,
    pub return_type: Option<TypeExpr<'src>>,
    pub body: Block<'src>,
}

#[derive(Debug)]
pub struct Parameter<'src> {
    pub mutability: Mutability,
    pub ty: TypeExpr<'src>,
    pub name: Name<'src>,
    pub span: Span,
}

/// Whether a binding was declared with `constant` or with `variable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutability {
    Constant,
    Variable,
}

/// A `type` declaration: a record with named fields.
#[derive(Debug)]
pub struct TypeDeclaration<'src> {
    pub name: Name<'src>,
    pub fields: Vec<Field<'src>>,
}

/// A typed name: a field of a `type` or of a `choice` variant.
#[derive(Debug)]
pub struct Field<'src> {
    pub ty: TypeExpr<'src>,
    pub name: Name<'src>,
    pub span: Span,
}

/// A `choice` declaration: a value that is exactly one of its variants.
#[derive(Debug)]
pub struct ChoiceDeclaration<'src> {
    pub name: Name<'src>,
    pub variants: Vec<Variant<'src>>,
}

#[derive(Debug)]
pub struct Variant<'src> {
    pub name: Name<'src>,
    pub fields: Vec<Field<'src>>,
    pub span: Span,
}

/// A type as written in the source, such as `list[integer32]` or `packed Shape`.
#[derive(Debug)]
pub struct TypeExpr<'src> {
    pub kind: TypeExprKind<'src>,
    pub span: Span,
}

#[derive(Debug)]
pub enum TypeExprKind<'src> {
    Named {
        path: Path<'src>,
        arguments: Vec<TypeArgument<'src>>,
    },
    Packed(Box<TypeExpr<'src>>),
}

/// What goes between the brackets of a type: another type, or a size as in `array[float32, 4]`.
#[derive(Debug)]
pub enum TypeArgument<'src> {
    Type(TypeExpr<'src>),
    Size { value: u64, span: Span },
}

/// The indented statements that belong to a declaration or statement.
#[derive(Debug)]
pub struct Block<'src> {
    pub statements: Vec<Statement<'src>>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Statement<'src> {
    pub kind: StatementKind<'src>,
    pub span: Span,
}

#[derive(Debug)]
pub enum StatementKind<'src> {
    Binding(Binding<'src>),
    Assignment {
        target: Expr<'src>,
        value: Expr<'src>,
    },
    Expression(Expr<'src>),
    If(IfStatement<'src>),
    While {
        condition: Expr<'src>,
        body: Block<'src>,
    },
    For {
        variable: Name<'src>,
        iterable: Expr<'src>,
        body: Block<'src>,
    },
    Match {
        scrutinee: Expr<'src>,
        arms: Vec<MatchArm<'src>>,
    },
    Return(Option<Expr<'src>>),
    Break,
    Continue,
    Assert(Expr<'src>),
}

/// A `variable` or `constant` declaration. The type is absent when it is inferred.
#[derive(Debug)]
pub struct Binding<'src> {
    pub mutability: Mutability,
    pub ty: Option<TypeExpr<'src>>,
    pub name: Name<'src>,
    pub value: Expr<'src>,
}

/// An `if` with its `else if` branches in order, then the optional `else` block.
#[derive(Debug)]
pub struct IfStatement<'src> {
    pub branches: Vec<ConditionalBranch<'src>>,
    pub otherwise: Option<Block<'src>>,
}

#[derive(Debug)]
pub struct ConditionalBranch<'src> {
    pub condition: Expr<'src>,
    pub body: Block<'src>,
}

#[derive(Debug)]
pub struct MatchArm<'src> {
    pub pattern: Pattern<'src>,
    pub body: Block<'src>,
    pub span: Span,
}

/// A `case` pattern: a variant name and the names its fields are bound to.
#[derive(Debug)]
pub struct Pattern<'src> {
    pub variant: Name<'src>,
    pub bindings: Vec<Name<'src>>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Expr<'src> {
    pub kind: ExprKind<'src>,
    pub span: Span,
}

#[derive(Debug)]
pub enum ExprKind<'src> {
    Integer(u64),
    Float(f64),
    Bool(bool),
    Text(Vec<TextPart<'src>>),
    Name(&'src str),
    Unary {
        operator: UnaryOperator,
        operand: Box<Expr<'src>>,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expr<'src>>,
        right: Box<Expr<'src>>,
    },
    Call(Box<Call<'src>>),
    Field {
        object: Box<Expr<'src>>,
        field: Name<'src>,
    },
    Index {
        object: Box<Expr<'src>>,
        index: Box<Expr<'src>>,
    },
}

impl Expr<'_> {
    /// True for the forms that can stand on the left of `=`: a name, a field, or an element.
    pub fn is_place(&self) -> bool {
        matches!(
            self.kind,
            ExprKind::Name(_) | ExprKind::Field { .. } | ExprKind::Index { .. }
        )
    }
}

/// A piece of a text literal: characters as written, or an interpolated expression.
#[derive(Debug)]
pub enum TextPart<'src> {
    Literal(String),
    Interpolation(Expr<'src>),
}

#[derive(Debug)]
pub struct Call<'src> {
    pub callee: Expr<'src>,
    pub arguments: Vec<Argument<'src>>,
    /// The `with` block passed as code (spec 7.7).
    pub block: Option<BlockArgument<'src>>,
}

#[derive(Debug)]
pub struct Argument<'src> {
    pub kind: ArgumentKind<'src>,
    pub span: Span,
}

#[derive(Debug)]
pub enum ArgumentKind<'src> {
    Positional(Expr<'src>),
    /// `name = value`, as in `Frog(name = "Kero")`.
    Named {
        name: Name<'src>,
        value: Expr<'src>,
    },
    /// `variable place`: the call may change the caller's binding.
    Mutable(Expr<'src>),
}

/// A block of code given to a call: `with` parameters, then the indented body.
#[derive(Debug)]
pub struct BlockArgument<'src> {
    pub parameters: Vec<Name<'src>>,
    pub body: Block<'src>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperator {
    Negate,
    Not,
}

impl UnaryOperator {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Negate => "-",
            Self::Not => "not",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperator {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

impl BinaryOperator {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Or => "or",
            Self::And => "and",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Remainder => "%",
        }
    }

    /// Comparisons do not chain: `a < b < c` is rejected instead of guessed at.
    pub const fn is_comparison(self) -> bool {
        matches!(
            self,
            Self::Equal
                | Self::NotEqual
                | Self::Less
                | Self::LessEqual
                | Self::Greater
                | Self::GreaterEqual
        )
    }
}
