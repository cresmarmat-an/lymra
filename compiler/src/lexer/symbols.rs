/*!
 * Punctuation and operators.
 *
 * Spellings from other languages that have an exact Lymra equivalent are
 * reported once and then read as that equivalent (`&&` as `and`), so one habit
 * costs one error instead of a cascade.
 */

use super::{Keyword, Lexer, TokenKind, errors, is_foreign_letter};
use crate::source::Span;

impl Lexer<'_> {
    pub(super) fn lex_symbol(&mut self, first: char, start: u32) {
        self.cursor.bump();
        let kind = match first {
            '(' => self.open_bracket(TokenKind::LeftParen),
            '[' => self.open_bracket(TokenKind::LeftBracket),
            ')' => self.close_bracket(TokenKind::RightParen),
            ']' => self.close_bracket(TokenKind::RightBracket),
            ',' => TokenKind::Comma,
            '.' => TokenKind::Dot,
            ':' => TokenKind::Colon,
            ';' => TokenKind::Semicolon,
            '=' => self.lex_equals(start),
            '!' => self.lex_bang(start),
            '<' => self.with_equals(TokenKind::LessEqual, TokenKind::Less),
            '>' => self.with_equals(TokenKind::GreaterEqual, TokenKind::Greater),
            '*' => self.with_equals(TokenKind::CompoundAssign, TokenKind::Star),
            '%' => self.with_equals(TokenKind::CompoundAssign, TokenKind::Percent),
            '+' if self.cursor.eat('+') => TokenKind::PlusPlus,
            '+' => self.with_equals(TokenKind::CompoundAssign, TokenKind::Plus),
            '-' if self.cursor.eat('-') => TokenKind::MinusMinus,
            '-' if self.cursor.eat('>') => TokenKind::Arrow,
            '-' => self.with_equals(TokenKind::CompoundAssign, TokenKind::Minus),
            '/' if self.cursor.starts_with("/") => return self.skip_foreign_line_comment(start),
            '/' if self.cursor.starts_with("*") => return self.skip_foreign_block_comment(start),
            '/' => self.with_equals(TokenKind::CompoundAssign, TokenKind::Slash),
            '&' if self.cursor.eat('&') => self.read_as_keyword(start, Keyword::And),
            '|' if self.cursor.eat('|') => self.read_as_keyword(start, Keyword::Or),
            '{' | '}' => return self.reject_brace(first, start),
            _ if is_foreign_letter(first) => return self.reject_foreign_name(start),
            _ => return self.reject_character(first, start),
        };
        self.push(kind, start);
    }

    fn open_bracket(&mut self, kind: TokenKind) -> TokenKind {
        self.bracket_depth += 1;
        kind
    }

    fn close_bracket(&mut self, kind: TokenKind) -> TokenKind {
        self.bracket_depth = self.bracket_depth.saturating_sub(1);
        kind
    }

    /// Picks between an operator and its form with a trailing `=`.
    fn with_equals(&mut self, with: TokenKind, without: TokenKind) -> TokenKind {
        if self.cursor.eat('=') { with } else { without }
    }

    fn lex_equals(&mut self, start: u32) -> TokenKind {
        if !self.cursor.eat('=') {
            return TokenKind::Assign;
        }
        if self.cursor.eat('=') {
            self.report_foreign_operator(start, "==", "==");
        }
        TokenKind::Equal
    }

    fn lex_bang(&mut self, start: u32) -> TokenKind {
        if !self.cursor.eat('=') {
            return self.read_as_keyword(start, Keyword::Not);
        }
        if self.cursor.eat('=') {
            self.report_foreign_operator(start, "!=", "!=");
        }
        TokenKind::NotEqual
    }

    /// Reports a foreign operator and continues as if the Lymra keyword had been written.
    fn read_as_keyword(&mut self, start: u32, keyword: Keyword) -> TokenKind {
        let replacement = self.spaced(start, keyword.as_str());
        self.report_foreign_operator(start, keyword.as_str(), &replacement);
        TokenKind::Keyword(keyword)
    }

    fn report_foreign_operator(&mut self, start: u32, lymra: &str, replacement: &str) {
        let span = Span::new(start, self.cursor.offset());
        let found = self.cursor.slice_from(start);
        self.diagnostics
            .push(errors::foreign_operator(found, lymra, span, replacement));
    }

    /// Pads a keyword with the spaces it needs to replace a symbol, as in `a&&b` to `a and b`.
    fn spaced(&self, start: u32, word: &str) -> String {
        let touches_before = self
            .cursor
            .character_before(start)
            .is_some_and(|character| !character.is_whitespace());
        let touches_after = self
            .cursor
            .peek()
            .is_some_and(|character| !character.is_whitespace());
        let before = if touches_before { " " } else { "" };
        let after = if touches_after { " " } else { "" };
        format!("{before}{word}{after}")
    }

    /// `//` is most often a comment from a C-family habit, so the rest of the line is skipped.
    fn skip_foreign_line_comment(&mut self, start: u32) {
        self.cursor.bump();
        let span = Span::new(start, self.cursor.offset());
        self.diagnostics.push(errors::foreign_line_comment(span));
        self.cursor.skip_to_line_end();
    }

    fn skip_foreign_block_comment(&mut self, start: u32) {
        self.cursor.bump();
        let span = Span::new(start, self.cursor.offset());
        self.diagnostics.push(errors::foreign_block_comment(span));
        let rest = self.cursor.rest();
        let length = rest.find("*/").map_or(rest.len(), |close| close + 2);
        let spans_lines = rest
            .get(..length)
            .is_some_and(|comment| comment.contains('\n'));
        self.cursor.advance_bytes(length);
        // Code after a comment that spans lines would be read with the wrong indentation.
        if spans_lines {
            self.cursor.skip_to_line_end();
        }
    }

    /**
     * Braces outside text are block delimiters from another language. Every
     * `{` is reported; the `}` that pairs with a reported `{` is dropped
     * without a second error.
     */
    fn reject_brace(&mut self, brace: char, start: u32) {
        let is_opening = brace == '{';
        if is_opening || !self.brace_reported {
            let span = Span::new(start, self.cursor.offset());
            self.diagnostics.push(errors::foreign_brace(brace, span));
        }
        self.brace_reported |= is_opening;
    }

    fn reject_character(&mut self, character: char, start: u32) {
        let span = Span::new(start, self.cursor.offset());
        self.diagnostics
            .push(errors::unexpected_character(character, span));
        self.push(TokenKind::Error, start);
    }
}
