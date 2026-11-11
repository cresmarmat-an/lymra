//! Names for tokens as they appear in error messages.

use crate::lexer::{Token, TokenKind};
use crate::source::SourceFile;

/// Describes a token the way a message refers to it, as in "found end of line".
pub(super) fn token(token: Token, source: &SourceFile) -> String {
    let text = source.slice(token.span);
    match token.kind {
        TokenKind::Identifier => format!("name `{text}`"),
        TokenKind::Integer(_) | TokenKind::Float(_) => format!("number `{text}`"),
        TokenKind::Keyword(keyword) => format!("keyword `{}`", keyword.as_str()),
        TokenKind::TextStart | TokenKind::TextSegment | TokenKind::TextEnd => "text".to_owned(),
        TokenKind::Directive => "a directive".to_owned(),
        TokenKind::Newline => "the end of the line".to_owned(),
        TokenKind::Indent => "an indented block".to_owned(),
        TokenKind::Dedent => "the end of the block".to_owned(),
        TokenKind::EndOfFile => "the end of the file".to_owned(),
        TokenKind::Error => "invalid source".to_owned(),
        // Punctuation reads best as its own spelling.
        _ => format!("`{text}`"),
    }
}

/// The spelling of a closing bracket.
pub(super) fn closer(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::RightBracket => "]",
        TokenKind::InterpolationEnd => "}",
        _ => ")",
    }
}
