//! Tokens: the words and punctuation the parser reads.

use crate::source::Span;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TokenKind {
    Identifier,
    Integer(u64),
    Float(f64),
    Keyword(Keyword),

    // A text literal is a `TextStart`, then segments and interpolations, then a `TextEnd`.
    TextStart,
    TextSegment,
    InterpolationStart,
    InterpolationEnd,
    TextEnd,

    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Dot,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,

    // Not Lymra syntax. Lexed so the parser can explain the Lymra spelling in context.
    Colon,
    Semicolon,
    Arrow,
    CompoundAssign,
    PlusPlus,
    MinusMinus,

    /// A whole `#!` line. The parser interprets its words.
    Directive,

    Newline,
    Indent,
    Dedent,
    EndOfFile,

    /// Stands in for source the lexer has already rejected.
    Error,
}

macro_rules! keywords {
    ($($variant:ident => $text:literal,)+) => {
        /// The reserved words of Lymra. All are whole English words (spec 2.5).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Keyword {
            $($variant,)+
        }

        impl Keyword {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }

            pub fn from_text(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

impl Keyword {
    /// True for keywords that start a line which an indented block follows.
    pub const fn opens_block(self) -> bool {
        matches!(
            self,
            Self::Function
                | Self::Type
                | Self::Choice
                | Self::Public
                | Self::If
                | Self::Else
                | Self::While
                | Self::For
                | Self::Match
                | Self::Case
        )
    }

    /**
     * True for keywords that can only begin a statement or a declaration.
     * None of them can continue an expression, so one at the start of a line
     * means a new statement has begun, even inside an unclosed bracket.
     */
    pub const fn begins_statement_only(self) -> bool {
        matches!(
            self,
            Self::Function
                | Self::Type
                | Self::Choice
                | Self::Use
                | Self::Public
                | Self::External
                | Self::Constant
                | Self::If
                | Self::Else
                | Self::While
                | Self::For
                | Self::Match
                | Self::Case
                | Self::Return
                | Self::Break
                | Self::Continue
                | Self::Assert
        )
    }
}

keywords! {
    Variable => "variable",
    Constant => "constant",
    Function => "function",
    Returns => "returns",
    Return => "return",
    Type => "type",
    Choice => "choice",
    Packed => "packed",
    Use => "use",
    Public => "public",
    If => "if",
    Else => "else",
    While => "while",
    For => "for",
    In => "in",
    Break => "break",
    Continue => "continue",
    Match => "match",
    Case => "case",
    And => "and",
    Or => "or",
    Not => "not",
    True => "true",
    False => "false",
    External => "external",
    Assert => "assert",
    With => "with",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_round_trip_through_their_spelling() {
        for keyword in [
            Keyword::Variable,
            Keyword::Returns,
            Keyword::With,
            Keyword::Not,
        ] {
            assert_eq!(Keyword::from_text(keyword.as_str()), Some(keyword));
        }
        assert_eq!(Keyword::from_text("var"), None);
        assert_eq!(Keyword::from_text("Variable"), None);
    }
}
