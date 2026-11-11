//! Getting back in step after a syntax error, so the lines that follow are still checked.

use super::{ParseResult, Parser, errors};
use crate::diagnostics::ErrorReported;
use crate::lexer::{Keyword, TokenKind};

impl Parser<'_, '_> {
    /**
     * Parses lines with `parse_line` until the enclosing block ends. A line
     * that fails is skipped together with the block it owns.
     */
    pub(super) fn parse_lines<T>(
        &mut self,
        parse_line: impl Fn(&mut Self) -> ParseResult<T>,
    ) -> Vec<T> {
        let mut lines = Vec::new();
        while !matches!(
            self.current().kind,
            TokenKind::Dedent | TokenKind::EndOfFile
        ) {
            let before = self.position;
            if self.at(TokenKind::Indent) {
                let span = self.current().span;
                self.record(errors::unexpected_indentation(span));
                self.skip_block();
            } else {
                match parse_line(self) {
                    Ok(line) => lines.push(line),
                    Err(ErrorReported) => self.recover(),
                }
            }
            // Every round has to consume a token, or a bug in a line parser would loop forever.
            if self.position == before {
                self.advance();
            }
        }
        lines
    }

    /// Skips the rest of a rejected line, the block under it, and any `else` that belonged to it.
    pub(super) fn recover(&mut self) {
        self.skip_line_and_block();
        // An `else` after a skipped `if` would only be reported as having no `if`.
        while self.at_keyword(Keyword::Else) {
            self.skip_line_and_block();
        }
    }

    fn skip_line_and_block(&mut self) {
        loop {
            match self.current().kind {
                TokenKind::EndOfFile | TokenKind::Dedent => return,
                TokenKind::Newline => {
                    self.advance();
                    break;
                }
                _ => {
                    self.advance();
                }
            }
        }
        if self.at(TokenKind::Indent) {
            self.skip_block();
        }
    }

    /// Skips an indented block, from its `Indent` through the matching `Dedent`.
    pub(super) fn skip_block(&mut self) {
        let mut depth = 0_u32;
        loop {
            match self.current().kind {
                TokenKind::Indent => depth += 1,
                TokenKind::Dedent => depth = depth.saturating_sub(1),
                TokenKind::EndOfFile => return,
                _ => {}
            }
            self.advance();
            if depth == 0 {
                return;
            }
        }
    }
}
