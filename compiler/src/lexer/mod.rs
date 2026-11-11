/*!
 * The lexer: turns source text into tokens.
 *
 * Lymra is indentation-sensitive, so besides words and punctuation the token
 * stream carries the block structure as `Newline`, `Indent` and `Dedent`
 * tokens. Inside parentheses and brackets, line breaks and indentation mean
 * nothing, which is how one statement continues over several lines.
 *
 * The lexer never stops at an error. It records a diagnostic, and where the
 * source cannot be read as a token it emits `TokenKind::Error`, so the parser
 * knows that statement was already rejected.
 */

mod cursor;
mod errors;
mod escape;
mod layout;
mod number;
mod symbols;
mod text;
mod token;
mod trivia;

pub use escape::unescape;
pub use token::{Keyword, Token, TokenKind};

use crate::diagnostics::Diagnostic;
use crate::source::{SourceFile, Span};
use cursor::Cursor;
use layout::{Indentation, Layout, LayoutChange};
use number::NumberValue;

const BYTE_ORDER_MARK: char = '\u{FEFF}';

/// The tokens of one file, always ending in `EndOfFile`, with the errors found on the way.
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn lex(source: &SourceFile) -> Lexed {
    let mut lexer = Lexer::new(source.text());
    lexer.run();
    Lexed {
        tokens: lexer.tokens,
        diagnostics: lexer.diagnostics,
    }
}

struct Lexer<'src> {
    cursor: Cursor<'src>,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
    layout: Layout,
    /// Open `(` and `[` on the current logical line. Line breaks are ignored while it is above zero.
    bracket_depth: u32,
    /// How many text literals deep the lexer is inside interpolations.
    interpolation_depth: u32,
    /// Set once a `{` outside text has been reported, so its `}` is dropped quietly.
    brace_reported: bool,
    /// Where the content of the last logical line ended. Dedents are placed here.
    last_line_end: u32,
    /// The line end at which unclosed text was last reported, to report nested literals once.
    unclosed_text_at: Option<u32>,
    /**
     * The indentation of the line being lexed, until its first token appears.
     * A line that holds only rejected characters never produces a token, and
     * then it must not open or close blocks either.
     */
    pending_indentation: Option<Indentation>,
    /// Index of the first token of the logical line being lexed.
    line_first_token: usize,
    /// Whether the previous logical line is one that an indented block follows.
    block_expected: bool,
}

impl<'src> Lexer<'src> {
    fn new(text: &'src str) -> Self {
        Self {
            cursor: Cursor::new(text),
            tokens: Vec::new(),
            diagnostics: Vec::new(),
            layout: Layout::new(),
            bracket_depth: 0,
            interpolation_depth: 0,
            brace_reported: false,
            last_line_end: 0,
            unclosed_text_at: None,
            pending_indentation: None,
            line_first_token: 0,
            block_expected: false,
        }
    }

    fn run(&mut self) {
        self.skip_file_prefix();
        while self.start_line() {
            self.lex_line();
        }
        for _ in 0..self.layout.close_all() {
            self.push_empty(TokenKind::Dedent, self.last_line_end);
        }
        self.push_empty(TokenKind::EndOfFile, self.cursor.offset());
    }

    /// Skips what may precede the first line: a byte order mark and a Unix shebang.
    fn skip_file_prefix(&mut self) {
        self.cursor.eat(BYTE_ORDER_MARK);
        if self.cursor.starts_with("#!/") {
            self.cursor.skip_to_line_end();
        }
    }

    /**
     * Skips blank and comment-only lines, then measures the indentation of the
     * first line that has content. Returns false at the end of the file.
     */
    fn start_line(&mut self) -> bool {
        loop {
            let indentation = self.measure_indentation();
            if self.cursor.starts_with("#!") {
                self.pending_indentation = Some(indentation);
                self.lex_directive();
                return true;
            }
            self.skip_trivia();
            if self.cursor.is_at_end() {
                return false;
            }
            if !self.cursor.eat_line_break() {
                self.pending_indentation = Some(indentation);
                return true;
            }
        }
    }

    /// Lexes tokens up to the line break that ends the logical line.
    fn lex_line(&mut self) {
        loop {
            self.skip_trivia();
            // Inside brackets `skip_trivia` consumes line breaks, so this ends the line only at depth zero.
            if self.cursor.is_at_line_end() {
                break;
            }
            self.lex_token();
        }
        self.bracket_depth = 0;
        // Indentation that is still pending means no token appeared: the line was all rejected source.
        if self.pending_indentation.take().is_some() {
            self.cursor.eat_line_break();
            return;
        }
        self.block_expected = self.line_opens_block();
        self.last_line_end = self.cursor.offset();
        self.cursor.eat_line_break();
        self.push(TokenKind::Newline, self.last_line_end);
    }

    fn measure_indentation(&mut self) -> Indentation {
        let start = self.cursor.offset();
        let (mut spaces, mut tabs) = (0, 0);
        loop {
            if self.cursor.eat(' ') {
                spaces += 1;
            } else if self.cursor.eat('\t') {
                tabs += 1;
            } else {
                break;
            }
        }
        Indentation {
            spaces,
            tabs,
            span: Span::new(start, self.cursor.offset()),
        }
    }

    /// Applies the pending indentation, if this is the first token of its line.
    fn start_line_tokens(&mut self) {
        if let Some(indentation) = self.pending_indentation.take() {
            self.apply_indentation(indentation);
        }
    }

    fn apply_indentation(&mut self, indentation: Indentation) {
        match self
            .layout
            .apply(indentation, self.block_expected, &mut self.diagnostics)
        {
            LayoutChange::Unchanged => {}
            LayoutChange::Indented => self.tokens.push(Token {
                kind: TokenKind::Indent,
                span: indentation.span,
            }),
            LayoutChange::Dedented(blocks) => {
                for _ in 0..blocks {
                    self.push_empty(TokenKind::Dedent, self.last_line_end);
                }
            }
        }
        self.line_first_token = self.tokens.len();
    }

    /// True if the line just lexed starts with a block keyword or hands a `with` block to a call.
    fn line_opens_block(&self) -> bool {
        let line = self.tokens.get(self.line_first_token..).unwrap_or_default();
        let starts_block = match line.first().map(|token| token.kind) {
            Some(TokenKind::Keyword(keyword)) => keyword.opens_block(),
            _ => false,
        };
        starts_block
            || line
                .iter()
                .any(|token| token.kind == TokenKind::Keyword(Keyword::With))
    }

    /// A directive is the `#!` line up to a trailing comment. The parser reads its words.
    fn lex_directive(&mut self) {
        let start = self.cursor.offset();
        self.cursor.skip_to_line_end();
        let line = self.cursor.slice_from(start);
        let follows_space = |index: usize| {
            line.get(..index)
                .is_some_and(|before| before.ends_with([' ', '\t']))
        };
        let comment_start = line
            .match_indices('#')
            .map(|(index, _)| index)
            .find(|&index| follows_space(index));
        let directive = line
            .get(..comment_start.unwrap_or(line.len()))
            .unwrap_or(line)
            .trim_end();
        let span = Span::new(start, start + directive.len() as u32);
        self.start_line_tokens();
        self.tokens.push(Token {
            kind: TokenKind::Directive,
            span,
        });
    }

    fn lex_token(&mut self) {
        let start = self.cursor.offset();
        match self.cursor.peek() {
            Some('a'..='z' | 'A'..='Z' | '_') => self.lex_word(start),
            Some('0'..='9') => self.lex_number(start),
            Some('"') => self.lex_text(start),
            Some('\'') => self.lex_single_quoted(start),
            Some(character) => self.lex_symbol(character, start),
            None => {}
        }
    }

    fn lex_word(&mut self, start: u32) {
        self.cursor.eat_while(is_name_character);
        if self.cursor.peek().is_some_and(is_foreign_letter) {
            return self.reject_foreign_name(start);
        }
        let word = self.cursor.slice_from(start);
        let kind = Keyword::from_text(word).map_or(TokenKind::Identifier, TokenKind::Keyword);
        self.push(kind, start);
    }

    /// Consumes a whole word that has letters outside ASCII and rejects it as one name.
    fn reject_foreign_name(&mut self, start: u32) {
        self.cursor
            .eat_while(|character| is_name_character(character) || is_foreign_letter(character));
        let span = Span::new(start, self.cursor.offset());
        self.diagnostics
            .push(errors::non_ascii_name(self.cursor.slice_from(start), span));
        self.push(TokenKind::Error, start);
    }

    fn lex_number(&mut self, start: u32) {
        let scanned = number::scan_number(self.cursor.rest());
        self.cursor.advance_bytes(scanned.length);
        let kind = match scanned.value {
            Ok(NumberValue::Integer(value)) => TokenKind::Integer(value),
            Ok(NumberValue::Float(value)) => TokenKind::Float(value),
            Err(error) => {
                let span = Span::new(start, self.cursor.offset());
                self.diagnostics.push(errors::invalid_number(error, span));
                TokenKind::Error
            }
        };
        self.push(kind, start);
    }

    /// Adds a token that runs from `start` to the cursor.
    fn push(&mut self, kind: TokenKind, start: u32) {
        self.start_line_tokens();
        self.tokens.push(Token {
            kind,
            span: Span::new(start, self.cursor.offset()),
        });
    }

    /// Adds a zero-width token, for structure that has no text of its own.
    fn push_empty(&mut self, kind: TokenKind, offset: u32) {
        self.start_line_tokens();
        self.tokens.push(Token {
            kind,
            span: Span::empty(offset),
        });
    }
}

fn is_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// A letter or digit from outside ASCII, which a programmer may expect to work in a name.
fn is_foreign_letter(character: char) -> bool {
    !character.is_ascii() && character.is_alphanumeric()
}

#[cfg(test)]
mod tests;
