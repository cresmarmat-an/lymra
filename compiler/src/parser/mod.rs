/*!
 * The parser: turns tokens into the syntax tree.
 *
 * It is a recursive-descent parser. A failure is recorded as a diagnostic
 * and then travels up as `ErrorReported` to whichever function owns a whole
 * line, which skips to the next line and keeps going. One mistake therefore
 * costs one error, and the rest of the file is still checked.
 *
 * Mistakes with an obvious meaning, such as `fn` for `function`, are recorded
 * and then parsed as if they had been written correctly.
 */

mod bindings;
mod describe;
mod directives;
mod errors;
mod expressions;
mod foreign;
mod items;
mod recovery;
mod statements;
mod types;

#[cfg(test)]
mod tests;

use crate::ast::{Module, Name};
use crate::diagnostics::{Diagnostic, ErrorReported, Help};
use crate::lexer::{Keyword, Token, TokenKind};
use crate::source::{SourceFile, Span};

/// How deeply expressions, types and blocks may nest. Bounds the parser's recursion.
pub(crate) const MAX_NESTING: u32 = 64;

/**
 * How many binary operators, calls, indexes and field accesses one expression
 * may hold. Each makes the expression a level deeper without the parser
 * recursing, so `MAX_NESTING` alone would not bound how deep the later
 * stages, which follow expressions recursively, have to go.
 */
pub const MAX_OPERATIONS: u32 = 1000;

type ParseResult<T> = Result<T, ErrorReported>;

/// The syntax tree of one file. It holds every declaration that parsed, even when others failed.
pub struct Parsed<'src> {
    pub module: Module<'src>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse<'src>(source: &'src SourceFile, tokens: &[Token]) -> Parsed<'src> {
    let mut parser = Parser {
        source,
        tokens,
        position: 0,
        diagnostics: Vec::new(),
        nesting: 0,
        operations: None,
        body_not_indented: false,
    };
    let module = parser.parse_module();
    Parsed {
        module,
        diagnostics: parser.diagnostics,
    }
}

struct Parser<'src, 'tokens> {
    source: &'src SourceFile,
    tokens: &'tokens [Token],
    position: usize,
    diagnostics: Vec<Diagnostic>,
    /// Current depth of nested expressions, types and blocks.
    nesting: u32,
    /// The operations counted toward `MAX_OPERATIONS` in the outermost expression being parsed, while there is one.
    operations: Option<u32>,
    /**
     * Set when a top-level declaration has no indented block. Its lines then
     * sit at the top level, where each would be reported as a stray statement
     * and bury the one error that matters.
     */
    body_not_indented: bool,
}

impl<'src> Parser<'src, '_> {
    fn parse_module(&mut self) -> Module<'src> {
        let directives = self.parse_directives();
        let mut items = self.parse_lines(Self::parse_item);
        // A block end cannot occur at the top level of a well-formed token stream. Skip one rather than trust that.
        while !self.at(TokenKind::EndOfFile) {
            self.advance();
            items.extend(self.parse_lines(Self::parse_item));
        }
        Module { directives, items }
    }

    /// The token at `index`. Past the end, it is the end of the file.
    fn token_at(&self, index: usize) -> Token {
        let end_of_file = Token {
            kind: TokenKind::EndOfFile,
            span: Span::empty(self.source.end()),
        };
        self.tokens.get(index).copied().unwrap_or(end_of_file)
    }

    fn current(&self) -> Token {
        self.token_at(self.position)
    }

    fn peek(&self) -> Token {
        self.token_at(self.position + 1)
    }

    /// Where the token before the current one ended.
    fn previous_end(&self) -> u32 {
        match self.position.checked_sub(1) {
            Some(previous) => self.token_at(previous).span.end,
            None => 0,
        }
    }

    /// Consumes the current token. The end of the file is never consumed.
    fn advance(&mut self) -> Token {
        let token = self.current();
        if token.kind != TokenKind::EndOfFile {
            self.position += 1;
        }
        token
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn at_keyword(&self, keyword: Keyword) -> bool {
        self.at(TokenKind::Keyword(keyword))
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        let matches = self.at(kind);
        if matches {
            self.advance();
        }
        matches
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> bool {
        self.eat(TokenKind::Keyword(keyword))
    }

    /// True where a statement may end: at the end of its line, or at a `;` that will be reported.
    fn at_statement_end(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Newline | TokenKind::EndOfFile | TokenKind::Semicolon
        )
    }

    fn text(&self, span: Span) -> &'src str {
        self.source.slice(span)
    }

    fn name(&self, token: Token) -> Name<'src> {
        Name {
            text: self.text(token.span),
            span: token.span,
        }
    }

    /// The span from the current token to the last token on its line. Empty at the end of a line.
    fn rest_of_line(&self) -> Span {
        let start = self.current().span.start;
        let mut end = start;
        let mut index = self.position;
        loop {
            let token = self.token_at(index);
            let ends_line = matches!(
                token.kind,
                TokenKind::Newline | TokenKind::EndOfFile | TokenKind::Indent | TokenKind::Dedent
            );
            if ends_line {
                return Span::new(start, end);
            }
            end = token.span.end;
            index += 1;
        }
    }

    /// Records an error that stops the current line from being parsed.
    fn report(&mut self, diagnostic: Diagnostic) -> ErrorReported {
        self.diagnostics.push(diagnostic);
        ErrorReported
    }

    /// Records an error for a mistake the parser can read past.
    fn record(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Reports that the current token is not what the grammar needs here.
    fn unexpected(&mut self, expected: &str, help: Help) -> ErrorReported {
        let found = self.current();
        // The lexer has already explained an `Error` token; a second message would only repeat it.
        if found.kind == TokenKind::Error {
            return ErrorReported;
        }
        let description = describe::token(found, self.source);
        self.report(errors::unexpected_token(
            expected,
            &description,
            found.span,
            help,
        ))
    }

    fn expect_name(&mut self, expected: &str) -> ParseResult<Name<'src>> {
        let token = self.current();
        match token.kind {
            TokenKind::Identifier => {
                self.advance();
                Ok(self.name(token))
            }
            TokenKind::Keyword(keyword) => Err(self.report(errors::keyword_as_name(
                keyword.as_str(),
                expected,
                token.span,
            ))),
            _ => Err(self.unexpected(expected, Help::text(format!("write {expected} here")))),
        }
    }

    /// Runs `parse` one nesting level deeper, or fails if the limit is reached.
    fn nested<T>(&mut self, parse: impl FnOnce(&mut Self) -> ParseResult<T>) -> ParseResult<T> {
        if self.nesting >= MAX_NESTING {
            let span = self.current().span;
            return Err(self.report(errors::nesting_too_deep(span)));
        }
        self.nesting += 1;
        let result = parse(self);
        self.nesting -= 1;
        result
    }

    /// Counts one operation toward `MAX_OPERATIONS`, or fails if the expression already holds that many.
    fn count_operation(&mut self) -> ParseResult<()> {
        let Some(operations) = self.operations.as_mut() else {
            return Ok(());
        };
        *operations += 1;
        if *operations > MAX_OPERATIONS {
            let span = self.current().span;
            return Err(self.report(errors::expression_too_long(span)));
        }
        Ok(())
    }

    /**
     * Parses a comma-separated list up to its closing bracket, which is
     * consumed and returned. `open` is the bracket that started the list.
     */
    fn parse_list<T>(
        &mut self,
        open: Token,
        close: TokenKind,
        parse_element: impl Fn(&mut Self) -> ParseResult<T>,
    ) -> ParseResult<(Vec<T>, Token)> {
        let mut elements = Vec::new();
        while !self.at(close) {
            elements.push(parse_element(self)?);
            if !self.at(TokenKind::Comma) {
                break;
            }
            let comma = self.advance();
            if self.at(close) {
                self.record(errors::trailing_comma(comma.span));
            }
        }
        let closer = describe::closer(close);
        let closing = self.expect_closing(open, close, &format!("`,` or `{closer}`"))?;
        Ok((elements, closing))
    }

    /// Consumes the bracket that closes `open`, or reports where it should have been.
    fn expect_closing(
        &mut self,
        open: Token,
        close: TokenKind,
        expected: &str,
    ) -> ParseResult<Token> {
        if self.at(close) {
            return Ok(self.advance());
        }
        let found = self.current();
        if found.kind == TokenKind::Error {
            return Err(ErrorReported);
        }
        let description = describe::token(found, self.source);
        let at_line_end = matches!(found.kind, TokenKind::Newline | TokenKind::EndOfFile);
        Err(self.report(errors::unclosed_bracket(
            expected,
            describe::closer(close),
            &description,
            found.span,
            open.span,
            at_line_end,
        )))
    }
}
