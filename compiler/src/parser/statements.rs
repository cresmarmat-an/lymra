//! Statements and the indented blocks that hold them.

use super::{MAX_NESTING, ParseResult, Parser, errors, foreign};
use crate::ast::{
    BinaryOperator, Block, BlockArgument, ConditionalBranch, Expr, ExprKind, IfStatement, MatchArm,
    Mutability, Pattern, Statement, StatementKind,
};
use crate::diagnostics::{ErrorReported, Help};
use crate::lexer::{Keyword, Token, TokenKind};
use crate::source::Span;

/// How a simple statement ended: at the end of its line, or with the `with` block of its last call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StatementEnd {
    Line,
    Block,
}

impl<'src> Parser<'src, '_> {
    /// Parses the block under a header. `header` is the line the block belongs to, for error messages.
    pub(super) fn parse_block(&mut self, header: Span) -> ParseResult<Block<'src>> {
        let statements = self.parse_indented_lines(header, Self::parse_statement)?;
        let span = match (statements.first(), statements.last()) {
            (Some(first), Some(last)) => first.span.to(last.span),
            _ => Span::empty(header.end),
        };
        Ok(Block { statements, span })
    }

    /**
     * Parses the indented lines under a header: the line break, the indent,
     * each line, and the dedent. On failure the cursor is left on the line
     * break, so recovery skips this line and its block and nothing more.
     */
    pub(super) fn parse_indented_lines<T>(
        &mut self,
        header: Span,
        parse_line: impl Fn(&mut Self) -> ParseResult<T>,
    ) -> ParseResult<Vec<T>> {
        self.skip_foreign_block_opener();
        if !self.at(TokenKind::Newline) {
            let help = Help::text(
                "end the line here and put what belongs to it on the next lines, indented one level",
            );
            return Err(self.unexpected("the end of the line", help));
        }
        let indent = self.peek();
        if indent.kind != TokenKind::Indent {
            self.body_not_indented = self.nesting == 0;
            return Err(self.report(errors::missing_block(header)));
        }
        if self.nesting >= MAX_NESTING {
            return Err(self.report(errors::nesting_too_deep(indent.span)));
        }
        self.advance();
        self.advance();
        self.nesting += 1;
        let lines = self.parse_lines(parse_line);
        self.nesting -= 1;
        self.eat(TokenKind::Dedent);
        Ok(lines)
    }

    /// Reports and skips a `:`, `then` or `do` that ends a block header in other languages.
    fn skip_foreign_block_opener(&mut self) {
        let token = self.current();
        let word = self.text(token.span);
        let is_opener = match token.kind {
            TokenKind::Colon => true,
            TokenKind::Identifier => foreign::is_block_opener(word),
            _ => false,
        };
        if is_opener && self.peek().kind == TokenKind::Newline {
            let removal = Span::new(self.previous_end(), token.span.end);
            self.record(errors::foreign_block_opener(token.span, word, removal));
            self.advance();
        }
    }

    pub(super) fn parse_statement(&mut self) -> ParseResult<Statement<'src>> {
        let token = self.current();
        match token.kind {
            TokenKind::Keyword(keyword) => self.parse_keyword_statement(keyword, token),
            TokenKind::Identifier => self.parse_identifier_statement(token),
            TokenKind::Directive => Err(self.report(errors::late_directive(token.span))),
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_keyword_statement(
        &mut self,
        keyword: Keyword,
        token: Token,
    ) -> ParseResult<Statement<'src>> {
        match keyword {
            Keyword::Variable => self.parse_binding_statement(Mutability::Variable),
            Keyword::Constant => self.parse_binding_statement(Mutability::Constant),
            Keyword::If => self.parse_if(),
            Keyword::While => self.parse_while(),
            Keyword::For => self.parse_for(),
            Keyword::Match => self.parse_match(),
            Keyword::Return => self.parse_return(),
            Keyword::Assert => self.parse_assert(),
            Keyword::Break => self.parse_word_statement(StatementKind::Break),
            Keyword::Continue => self.parse_word_statement(StatementKind::Continue),
            Keyword::Else => Err(self.report(errors::stray_else(token.span))),
            Keyword::Case => Err(self.report(errors::stray_case(token.span))),
            Keyword::Function
            | Keyword::Type
            | Keyword::Choice
            | Keyword::Use
            | Keyword::Public
            | Keyword::External => Err(self.report(errors::declaration_in_function(
                keyword.as_str(),
                token.span,
            ))),
            _ => self.parse_expression_statement(),
        }
    }

    /// A statement that starts with a plain word: a keyword from another language, a typo, or an expression.
    fn parse_identifier_statement(&mut self, token: Token) -> ParseResult<Statement<'src>> {
        let word = self.text(token.span);
        let next = self.peek().kind;
        if next == TokenKind::Identifier {
            if let Some(mutability) = foreign::binding_keyword(word) {
                let lymra = match mutability {
                    Mutability::Constant => Keyword::Constant,
                    Mutability::Variable => Keyword::Variable,
                };
                self.record(errors::foreign_binding_keyword(
                    token.span,
                    word,
                    lymra.as_str(),
                ));
                return self.parse_binding_statement(mutability);
            }
        }
        // Two values side by side are never an expression, so a word before one is likely a keyword typo.
        let value_follows = matches!(
            next,
            TokenKind::Identifier
                | TokenKind::Integer(_)
                | TokenKind::Float(_)
                | TokenKind::TextStart
        );
        if value_follows {
            if let Some(keyword) = foreign::misspelled(word, &foreign::STATEMENT_KEYWORDS) {
                return Err(self.report(errors::misspelled_keyword(
                    token.span,
                    word,
                    keyword.as_str(),
                )));
            }
        }
        self.parse_expression_statement()
    }

    fn parse_binding_statement(&mut self, mutability: Mutability) -> ParseResult<Statement<'src>> {
        let (binding, end, span) = self.parse_binding(mutability)?;
        self.finish_statement(end)?;
        Ok(Statement {
            kind: StatementKind::Binding(binding),
            span,
        })
    }

    /// Parses a statement that starts with an expression: a call, an assignment, or a mistake that looks like one.
    fn parse_expression_statement(&mut self) -> ParseResult<Statement<'src>> {
        let target = self.parse_expression()?;
        let token = self.current();
        match token.kind {
            TokenKind::Assign => self.parse_assignment(target),
            TokenKind::CompoundAssign => Err(self.reject_compound_assignment(&target, token)),
            TokenKind::PlusPlus | TokenKind::MinusMinus => {
                Err(self.reject_increment(&target, token))
            }
            // `string b = a`: a type and a name before `=` is a declaration that lost its keyword.
            TokenKind::Identifier if self.peek().kind == TokenKind::Assign => Err(self.report(
                errors::declaration_without_keyword(target.span.to(token.span)),
            )),
            _ => {
                let (expr, end) = self.attach_block(target)?;
                self.finish_statement(end)?;
                Ok(Statement {
                    span: expr.span,
                    kind: StatementKind::Expression(expr),
                })
            }
        }
    }

    fn parse_assignment(&mut self, target: Expr<'src>) -> ParseResult<Statement<'src>> {
        self.advance();
        if !target.is_place() {
            return Err(self.report(errors::invalid_assignment_target(target.span)));
        }
        let (value, end) = self.parse_value()?;
        self.finish_statement(end)?;
        Ok(Statement {
            span: target.span.to(value.span),
            kind: StatementKind::Assignment { target, value },
        })
    }

    /// `count += 1` is spelled `count = count + 1`. The right side gets parentheses when it is more than one token.
    fn reject_compound_assignment(
        &mut self,
        target: &Expr<'src>,
        operator: Token,
    ) -> ErrorReported {
        self.advance();
        let value = self.rest_of_line();
        let value_text = self.text(value);
        let is_single_token = self.peek().kind == TokenKind::Newline;
        let operand = if is_single_token {
            value_text.to_owned()
        } else {
            format!("({value_text})")
        };
        let symbol = self.text(operator.span);
        let arithmetic = symbol.trim_end_matches('=');
        let target_text = self.text(target.span);
        let rewritten = format!("{target_text} = {target_text} {arithmetic} {operand}");
        let statement = target.span.to(value);
        self.report(errors::foreign_assignment_operator(
            operator.span,
            symbol,
            statement,
            rewritten,
        ))
    }

    /// `count++` is spelled `count = count + 1`.
    fn reject_increment(&mut self, target: &Expr<'src>, operator: Token) -> ErrorReported {
        let symbol = self.text(operator.span);
        let arithmetic = if operator.kind == TokenKind::PlusPlus {
            "+"
        } else {
            "-"
        };
        let target_text = self.text(target.span);
        let rewritten = format!("{target_text} = {target_text} {arithmetic} 1");
        let statement = target.span.to(operator.span);
        self.report(errors::foreign_assignment_operator(
            operator.span,
            symbol,
            statement,
            rewritten,
        ))
    }

    fn parse_if(&mut self) -> ParseResult<Statement<'src>> {
        let keyword = self.advance();
        let mut branches = vec![self.parse_branch(keyword.span)?];
        let mut otherwise = None;
        loop {
            let token = self.current();
            if self.eat_keyword(Keyword::Else) {
                if self.eat_keyword(Keyword::If) {
                    branches.push(self.parse_branch(token.span)?);
                    continue;
                }
                otherwise = Some(self.parse_block(token.span)?);
            } else if token.kind == TokenKind::Identifier
                && foreign::is_else_if(self.text(token.span))
            {
                self.record(errors::foreign_keyword(
                    token.span,
                    self.text(token.span),
                    "else if",
                ));
                self.advance();
                branches.push(self.parse_branch(token.span)?);
                continue;
            }
            break;
        }
        let last_block = otherwise
            .as_ref()
            .or(branches.last().map(|branch| &branch.body));
        let span = last_block.map_or(keyword.span, |block| keyword.span.to(block.span));
        Ok(Statement {
            kind: StatementKind::If(IfStatement {
                branches,
                otherwise,
            }),
            span,
        })
    }

    /// Parses a condition and its block. `start` is where the branch began, for error messages.
    fn parse_branch(&mut self, start: Span) -> ParseResult<ConditionalBranch<'src>> {
        let condition = self.parse_condition()?;
        let body = self.parse_block(start.to(condition.span))?;
        Ok(ConditionalBranch { condition, body })
    }

    /// Parses a condition. A `=` where `==` was meant is reported and read as a comparison.
    fn parse_condition(&mut self) -> ParseResult<Expr<'src>> {
        let condition = self.parse_expression()?;
        if !self.at(TokenKind::Assign) {
            return Ok(condition);
        }
        let assign = self.advance();
        self.record(errors::assignment_in_condition(assign.span));
        let right = self.parse_expression()?;
        let span = condition.span.to(right.span);
        let kind = ExprKind::Binary {
            operator: BinaryOperator::Equal,
            left: Box::new(condition),
            right: Box::new(right),
        };
        Ok(Expr { kind, span })
    }

    fn parse_while(&mut self) -> ParseResult<Statement<'src>> {
        let keyword = self.advance();
        let condition = self.parse_condition()?;
        let body = self.parse_block(keyword.span.to(condition.span))?;
        let span = keyword.span.to(body.span);
        Ok(Statement {
            kind: StatementKind::While { condition, body },
            span,
        })
    }

    fn parse_for(&mut self) -> ParseResult<Statement<'src>> {
        let keyword = self.advance();
        let variable = self.expect_name("a name for each item")?;
        if !self.eat_keyword(Keyword::In) {
            return Err(self.unexpected("`in`", Help::text("write a loop as `for item in items`")));
        }
        let iterable = self.parse_expression()?;
        let body = self.parse_block(keyword.span.to(iterable.span))?;
        let span = keyword.span.to(body.span);
        Ok(Statement {
            kind: StatementKind::For {
                variable,
                iterable,
                body,
            },
            span,
        })
    }

    fn parse_match(&mut self) -> ParseResult<Statement<'src>> {
        let keyword = self.advance();
        let scrutinee = self.parse_expression()?;
        let header = keyword.span.to(scrutinee.span);
        let arms = self.parse_indented_lines(header, Self::parse_match_arm)?;
        let span = arms.last().map_or(header, |arm| header.to(arm.span));
        Ok(Statement {
            kind: StatementKind::Match { scrutinee, arms },
            span,
        })
    }

    fn parse_match_arm(&mut self) -> ParseResult<MatchArm<'src>> {
        if !self.at_keyword(Keyword::Case) {
            let help = Help::text(
                "each line of a `match` is `case`, a variant name, and an indented block",
            );
            return Err(self.unexpected("`case`", help));
        }
        let keyword = self.advance();
        let pattern = self.parse_pattern()?;
        let body = self.parse_block(keyword.span.to(pattern.span))?;
        let span = keyword.span.to(body.span);
        Ok(MatchArm {
            pattern,
            body,
            span,
        })
    }

    fn parse_pattern(&mut self) -> ParseResult<Pattern<'src>> {
        let variant = self.expect_name("a variant name")?;
        let mut span = variant.span;
        let mut bindings = Vec::new();
        if self.at(TokenKind::LeftParen) {
            let open = self.advance();
            let parse_binding_name = |parser: &mut Self| parser.expect_name("a name for the field");
            let (names, close) =
                self.parse_list(open, TokenKind::RightParen, parse_binding_name)?;
            bindings = names;
            span = span.to(close.span);
        }
        Ok(Pattern {
            variant,
            bindings,
            span,
        })
    }

    fn parse_return(&mut self) -> ParseResult<Statement<'src>> {
        let keyword = self.advance();
        if self.at_statement_end() {
            self.end_statement()?;
            return Ok(Statement {
                kind: StatementKind::Return(None),
                span: keyword.span,
            });
        }
        let (value, end) = self.parse_value()?;
        self.finish_statement(end)?;
        Ok(Statement {
            span: keyword.span.to(value.span),
            kind: StatementKind::Return(Some(value)),
        })
    }

    fn parse_assert(&mut self) -> ParseResult<Statement<'src>> {
        let keyword = self.advance();
        let condition = self.parse_expression()?;
        self.end_statement()?;
        Ok(Statement {
            span: keyword.span.to(condition.span),
            kind: StatementKind::Assert(condition),
        })
    }

    /// Parses a statement that is one keyword and nothing else: `break` or `continue`.
    fn parse_word_statement(&mut self, kind: StatementKind<'src>) -> ParseResult<Statement<'src>> {
        let keyword = self.advance();
        self.end_statement()?;
        Ok(Statement {
            kind,
            span: keyword.span,
        })
    }

    /// Parses the expression that ends a simple statement, with its `with` block if it has one.
    pub(super) fn parse_value(&mut self) -> ParseResult<(Expr<'src>, StatementEnd)> {
        let value = self.parse_expression()?;
        self.attach_block(value)
    }

    /**
     * Gives a `with` block to the call it follows (spec 7.7). The block is
     * an argument of that one call, so the call has to be the whole
     * expression: in `1 + f(x) with n` it would be unclear what gets the block.
     */
    fn attach_block(&mut self, value: Expr<'src>) -> ParseResult<(Expr<'src>, StatementEnd)> {
        if !self.at_keyword(Keyword::With) {
            return Ok((value, StatementEnd::Line));
        }
        let with = self.current();
        let Expr {
            kind: ExprKind::Call(mut call),
            span,
        } = value
        else {
            return Err(self.report(errors::misplaced_with(with.span)));
        };
        let block = self.parse_block_argument()?;
        let span = span.to(block.span);
        call.block = Some(block);
        Ok((
            Expr {
                kind: ExprKind::Call(call),
                span,
            },
            StatementEnd::Block,
        ))
    }

    fn parse_block_argument(&mut self) -> ParseResult<BlockArgument<'src>> {
        let with = self.advance();
        let mut header = with.span;
        let mut parameters = Vec::new();
        while self.at(TokenKind::Identifier) {
            let parameter = self.expect_name("a block parameter")?;
            header = header.to(parameter.span);
            parameters.push(parameter);
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        let body = self.parse_block(header)?;
        let span = with.span.to(body.span);
        Ok(BlockArgument {
            parameters,
            body,
            span,
        })
    }

    pub(super) fn finish_statement(&mut self, end: StatementEnd) -> ParseResult<()> {
        match end {
            StatementEnd::Line => self.end_statement(),
            // The block's own lines already consumed the line break.
            StatementEnd::Block => Ok(()),
        }
    }

    /// Consumes the end of a statement's line. A `;` there is reported and skipped.
    pub(super) fn end_statement(&mut self) -> ParseResult<()> {
        let token = self.current();
        match token.kind {
            TokenKind::Newline => {
                self.advance();
                Ok(())
            }
            TokenKind::EndOfFile => Ok(()),
            TokenKind::Semicolon => {
                let ends_line = self.peek().kind == TokenKind::Newline;
                self.record(errors::semicolon(token.span, ends_line));
                self.advance();
                self.eat(TokenKind::Newline);
                Ok(())
            }
            _ => Err(self.unexpected(
                "the end of the line",
                Help::text("put each statement on its own line"),
            )),
        }
    }
}
