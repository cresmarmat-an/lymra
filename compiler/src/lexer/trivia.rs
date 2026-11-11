//! Whitespace and comments: everything between tokens.

use super::{Keyword, Lexer, errors, is_name_character};
use crate::source::Span;

const COMMENT_OPEN: &str = "#[";
const COMMENT_CLOSE: &str = "]#";

/// Characters that take no width on screen, usually pasted in by accident.
const ZERO_WIDTH: [char; 5] = ['\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'];

fn is_invisible(character: char) -> bool {
    (character.is_whitespace() && !character.is_ascii()) || ZERO_WIDTH.contains(&character)
}

impl Lexer<'_> {
    /// Skips spaces and comments. Line breaks are skipped only inside brackets.
    pub(super) fn skip_trivia(&mut self) {
        loop {
            match self.cursor.peek() {
                Some(' ' | '\t') => {
                    self.cursor.bump();
                }
                // A carriage return that is not part of a line break is plain whitespace.
                Some('\r') if !self.cursor.starts_with("\r\n") => {
                    self.cursor.bump();
                }
                Some('\n' | '\r')
                    if self.bracket_depth > 0 && !self.next_line_begins_statement() =>
                {
                    self.cursor.eat_line_break();
                }
                Some('#') if self.cursor.starts_with(COMMENT_OPEN) => self.skip_multiline_comment(),
                Some('#') => self.cursor.skip_to_line_end(),
                Some(character) if is_invisible(character) => self.reject_invisible(character),
                _ => return,
            }
        }
    }

    /**
     * Inside brackets a line break normally continues the statement. If the
     * next line starts with a keyword that can only begin a statement, the
     * bracket was left open by mistake. Ending the line there keeps the error
     * on one statement instead of letting it swallow the rest of the file.
     */
    fn next_line_begins_statement(&self) -> bool {
        let next_line = self
            .cursor
            .rest()
            .trim_start_matches(['\r', '\n'])
            .trim_start_matches([' ', '\t']);
        let word_length = next_line
            .find(|character| !is_name_character(character))
            .unwrap_or(next_line.len());
        let first_word = next_line.get(..word_length).unwrap_or_default();
        Keyword::from_text(first_word).is_some_and(Keyword::begins_statement_only)
    }

    /// Skips a `#[ ... ]#` comment. These nest, so commented-out code may contain comments.
    fn skip_multiline_comment(&mut self) {
        let start = self.cursor.offset();
        self.cursor.advance_bytes(COMMENT_OPEN.len());
        let mut depth = 1_u32;
        let mut spans_lines = false;
        while depth > 0 {
            if self.cursor.eat_str(COMMENT_OPEN) {
                depth += 1;
            } else if self.cursor.eat_str(COMMENT_CLOSE) {
                depth -= 1;
            } else {
                match self.cursor.bump() {
                    Some('\n') => spans_lines = true,
                    Some(_) => {}
                    None => {
                        let opening = Span::new(start, start + COMMENT_OPEN.len() as u32);
                        self.diagnostics.push(errors::unterminated_comment(opening));
                        return;
                    }
                }
            }
        }
        if spans_lines {
            self.require_line_end_after_comment();
        }
    }

    /**
     * A comment that spans lines has to end its line. Code after the closing
     * `]#` would sit where a reader does not look for it, and its indentation
     * would be measured from inside the comment.
     */
    fn require_line_end_after_comment(&mut self) {
        let close_end = self.cursor.offset();
        let closing = Span::new(close_end - COMMENT_CLOSE.len() as u32, close_end);
        self.cursor
            .eat_while(|character| character == ' ' || character == '\t');
        if self.cursor.is_at_line_end() || self.cursor.starts_with("#") {
            return;
        }
        let code_start = self.cursor.offset();
        self.cursor.skip_to_line_end();
        let code_length = self.cursor.slice_from(code_start).trim_end().len() as u32;
        let code = Span::new(code_start, code_start + code_length);
        self.diagnostics
            .push(errors::code_after_comment(code, closing));
    }

    fn reject_invisible(&mut self, character: char) {
        let start = self.cursor.offset();
        self.cursor.bump();
        let span = Span::new(start, self.cursor.offset());
        self.diagnostics
            .push(errors::invisible_character(character, span));
    }
}
