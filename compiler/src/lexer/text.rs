/*!
 * Text literals: runs of characters with `{...}` interpolations between them.
 *
 * A literal becomes a balanced token sequence: `TextStart`, then
 * `TextSegment`s and `InterpolationStart ... InterpolationEnd` groups, then
 * `TextEnd`. The sequence stays balanced even when the literal is malformed,
 * with an `Error` token marking the spot, so the parser needs no special cases.
 */

use super::{Lexer, TokenKind, errors, escape};
use crate::source::Span;

/// How many text literals may nest through interpolations. Bounds the lexer's recursion.
pub(super) const MAX_INTERPOLATION_DEPTH: u32 = 8;

impl Lexer<'_> {
    pub(super) fn lex_text(&mut self, start: u32) {
        self.cursor.bump();
        self.push(TokenKind::TextStart, start);
        self.lex_text_body('"', start);
    }

    /**
     * Text in single quotes is a habit from other languages. When the quote
     * is closed on the same line, report it once and read it as text, so the
     * rest of the statement is still checked.
     */
    pub(super) fn lex_single_quoted(&mut self, start: u32) {
        let line = self.cursor.rest().lines().next().unwrap_or_default();
        let closing_quote = line.get(1..).and_then(|after_quote| after_quote.find('\''));
        let Some(content_length) = closing_quote else {
            self.cursor.bump();
            let span = Span::new(start, self.cursor.offset());
            self.diagnostics
                .push(errors::unexpected_character('\'', span));
            return self.push(TokenKind::Error, start);
        };

        let content = line.get(1..=content_length).unwrap_or_default();
        let replacement = (!content.contains('"')).then(|| format!("\"{content}\""));
        let span = Span::new(start, start + content_length as u32 + 2);
        self.diagnostics
            .push(errors::single_quoted_text(span, replacement));

        self.cursor.bump();
        self.push(TokenKind::TextStart, start);
        self.lex_text_body('\'', start);
    }

    /// Lexes segments and interpolations up to the closing quote, which it consumes.
    fn lex_text_body(&mut self, quote: char, opening: u32) {
        let mut segment_start = self.cursor.offset();
        loop {
            if self.cursor.is_at_line_end() {
                self.push_segment(segment_start);
                /*
                 * Inside an interpolation, a quote whose text never ends is
                 * most likely the end of the outer text with the `}` missing
                 * before it. The interpolation reports that instead.
                 */
                if self.interpolation_depth == 0 {
                    let opening_quote = Span::new(opening, opening + 1);
                    self.report_unclosed(errors::unterminated_text(
                        opening_quote,
                        self.cursor.offset(),
                    ));
                }
                return self.close_text_after_error();
            }
            match self.cursor.peek() {
                Some(character) if character == quote => {
                    self.push_segment(segment_start);
                    let quote_start = self.cursor.offset();
                    self.cursor.bump();
                    return self.push(TokenKind::TextEnd, quote_start);
                }
                Some('\\') => self.lex_escape(),
                Some('{') if self.cursor.starts_with("{{") => self.cursor.advance_bytes(2),
                Some('{') => {
                    self.push_segment(segment_start);
                    if !self.lex_interpolation() {
                        return self.close_text_after_error();
                    }
                    segment_start = self.cursor.offset();
                }
                Some('}') if self.cursor.starts_with("}}") => self.cursor.advance_bytes(2),
                Some('}') => {
                    let brace = self.cursor.offset();
                    self.cursor.bump();
                    self.diagnostics
                        .push(errors::unmatched_brace(Span::new(brace, brace + 1)));
                }
                _ => {
                    self.cursor.bump();
                }
            }
        }
    }

    fn push_segment(&mut self, start: u32) {
        if start < self.cursor.offset() {
            self.push(TokenKind::TextSegment, start);
        }
    }

    fn lex_escape(&mut self) {
        let start = self.cursor.offset();
        self.cursor.bump();
        // A backslash at the end of the line is covered by the unclosed-text error.
        if self.cursor.is_at_line_end() {
            return;
        }
        let Some(escaped) = self.cursor.bump() else {
            return;
        };
        if escape::escape_value(escaped).is_none() {
            let span = Span::new(start, self.cursor.offset());
            self.diagnostics.push(errors::unknown_escape(escaped, span));
        }
    }

    /// Lexes `{ expression }` inside text. Returns false if the line ended before the closing brace.
    fn lex_interpolation(&mut self) -> bool {
        let start = self.cursor.offset();
        self.cursor.bump();
        self.push(TokenKind::InterpolationStart, start);
        let opening_brace = Span::new(start, start + 1);

        if self.interpolation_depth >= MAX_INTERPOLATION_DEPTH {
            self.cursor.skip_to_line_end();
            self.report_unclosed(errors::interpolation_too_deep(opening_brace));
            return self.close_interpolation_after_error();
        }

        // Brackets inside the interpolation must not leak into the line around the text.
        let outer_brackets = std::mem::take(&mut self.bracket_depth);
        self.interpolation_depth += 1;
        let closed = loop {
            self.cursor
                .eat_while(|character| character == ' ' || character == '\t');
            if self.cursor.is_at_line_end() {
                self.report_unclosed(errors::unterminated_interpolation(opening_brace));
                break self.close_interpolation_after_error();
            }
            if self.cursor.peek() == Some('}') {
                let brace = self.cursor.offset();
                self.cursor.bump();
                self.push(TokenKind::InterpolationEnd, brace);
                break true;
            }
            self.lex_token();
        };
        self.interpolation_depth -= 1;
        self.bracket_depth = outer_brackets;
        closed
    }

    /**
     * Records an error about text that runs off the end of its line. When
     * literals are nested, each enclosing one hits the same line end; only
     * the innermost report is kept.
     */
    fn report_unclosed(&mut self, diagnostic: crate::diagnostics::Diagnostic) {
        let line_end = self.cursor.offset();
        if self.unclosed_text_at != Some(line_end) {
            self.unclosed_text_at = Some(line_end);
            self.diagnostics.push(diagnostic);
        }
    }

    fn close_text_after_error(&mut self) {
        let offset = self.cursor.offset();
        self.push_empty(TokenKind::Error, offset);
        self.push_empty(TokenKind::TextEnd, offset);
    }

    fn close_interpolation_after_error(&mut self) -> bool {
        let offset = self.cursor.offset();
        self.push_empty(TokenKind::Error, offset);
        self.push_empty(TokenKind::InterpolationEnd, offset);
        false
    }
}
