/*!
 * Expressions, parsed by precedence climbing.
 *
 * From loosest to tightest: `or`, `and`, `not`, comparisons, `+ -`,
 * `* / %`, unary `-`, then calls, indexing and field access. Binary operators
 * group to the left, and comparisons do not chain.
 */

use super::{ParseResult, Parser, describe, errors};
use crate::ast::{
    Argument, ArgumentKind, BinaryOperator, Call, Expr, ExprKind, TextPart, UnaryOperator,
};
use crate::diagnostics::{Diagnostic, ErrorReported, Help};
use crate::lexer::{Keyword, Token, TokenKind, unescape};

/// The operand of `not` takes every operator that binds tighter than `and`.
const NOT_OPERAND_PRECEDENCE: u8 = 3;
const ADDITIVE_PRECEDENCE: u8 = 5;

fn binary_operator(kind: TokenKind) -> Option<BinaryOperator> {
    let operator = match kind {
        TokenKind::Keyword(Keyword::Or) => BinaryOperator::Or,
        TokenKind::Keyword(Keyword::And) => BinaryOperator::And,
        TokenKind::Equal => BinaryOperator::Equal,
        TokenKind::NotEqual => BinaryOperator::NotEqual,
        TokenKind::Less => BinaryOperator::Less,
        TokenKind::LessEqual => BinaryOperator::LessEqual,
        TokenKind::Greater => BinaryOperator::Greater,
        TokenKind::GreaterEqual => BinaryOperator::GreaterEqual,
        TokenKind::Plus => BinaryOperator::Add,
        TokenKind::Minus => BinaryOperator::Subtract,
        TokenKind::Star => BinaryOperator::Multiply,
        TokenKind::Slash => BinaryOperator::Divide,
        TokenKind::Percent => BinaryOperator::Remainder,
        _ => return None,
    };
    Some(operator)
}

/// How tightly an operator binds. A higher number binds tighter.
fn precedence(operator: BinaryOperator) -> u8 {
    match operator {
        BinaryOperator::Or => 1,
        BinaryOperator::And => 2,
        BinaryOperator::Equal
        | BinaryOperator::NotEqual
        | BinaryOperator::Less
        | BinaryOperator::LessEqual
        | BinaryOperator::Greater
        | BinaryOperator::GreaterEqual => 4,
        BinaryOperator::Add | BinaryOperator::Subtract => ADDITIVE_PRECEDENCE,
        BinaryOperator::Multiply | BinaryOperator::Divide | BinaryOperator::Remainder => 6,
    }
}

impl<'src> Parser<'src, '_> {
    pub(super) fn parse_expression(&mut self) -> ParseResult<Expr<'src>> {
        self.nested(|parser| parser.parse_binary(0))
    }

    /// Parses operators that bind at least as tightly as `minimum`.
    fn parse_binary(&mut self, minimum: u8) -> ParseResult<Expr<'src>> {
        let mut left = self.parse_prefix()?;
        let mut previous_was_comparison = false;
        let mut chain_reported = false;
        while let Some(operator) = binary_operator(self.current().kind) {
            let binding = precedence(operator);
            if binding < minimum {
                break;
            }
            let operator_token = self.advance();
            // One level tighter on the right makes operators of equal precedence group to the left.
            let right = self.parse_binary(binding + 1)?;

            if operator.is_comparison() && previous_was_comparison && !chain_reported {
                let diagnostic = self.chained_comparison(&left, operator_token, &right);
                self.record(diagnostic);
                chain_reported = true;
            }
            previous_was_comparison = operator.is_comparison();

            let span = left.span.to(right.span);
            let kind = ExprKind::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            };
            left = Expr { kind, span };
        }
        Ok(left)
    }

    /// Builds the error for `a < b < c`, with the rewrite `a < b and b < c`.
    fn chained_comparison(
        &self,
        first: &Expr<'src>,
        second_operator: Token,
        last: &Expr<'src>,
    ) -> Diagnostic {
        let whole = first.span.to(last.span);
        // The operand in the middle is shared by both comparisons.
        let shared = match &first.kind {
            ExprKind::Binary { right, .. } => right.span,
            _ => first.span,
        };
        let rewritten = format!(
            "{} and {} {} {}",
            self.text(first.span),
            self.text(shared),
            self.text(second_operator.span),
            self.text(last.span)
        );
        errors::chained_comparison(whole, rewritten)
    }

    fn parse_prefix(&mut self) -> ParseResult<Expr<'src>> {
        let token = self.current();
        let (operator, operand) = match token.kind {
            TokenKind::Keyword(Keyword::Not) => {
                self.advance();
                (
                    UnaryOperator::Not,
                    self.nested(|parser| parser.parse_binary(NOT_OPERAND_PRECEDENCE))?,
                )
            }
            TokenKind::Minus => {
                self.advance();
                (UnaryOperator::Negate, self.nested(Self::parse_prefix)?)
            }
            _ => return self.parse_postfix(),
        };
        let span = token.span.to(operand.span);
        Ok(Expr {
            kind: ExprKind::Unary {
                operator,
                operand: Box::new(operand),
            },
            span,
        })
    }

    /// Parses a primary expression and the calls, indexes and field accesses that follow it.
    fn parse_postfix(&mut self) -> ParseResult<Expr<'src>> {
        let mut expr = self.parse_primary()?;
        loop {
            expr = match self.current().kind {
                TokenKind::LeftParen => self.parse_call(expr)?,
                TokenKind::LeftBracket => self.parse_index(expr)?,
                TokenKind::Dot => self.parse_field_access(expr)?,
                _ => return Ok(expr),
            };
        }
    }

    fn parse_call(&mut self, callee: Expr<'src>) -> ParseResult<Expr<'src>> {
        let open = self.advance();
        let (arguments, close) =
            self.parse_list(open, TokenKind::RightParen, Self::parse_argument)?;
        let span = callee.span.to(close.span);
        Ok(Expr {
            kind: ExprKind::Call(Box::new(Call {
                callee,
                arguments,
                block: None,
            })),
            span,
        })
    }

    fn parse_argument(&mut self) -> ParseResult<Argument<'src>> {
        let first = self.current();
        if self.eat_keyword(Keyword::Variable) {
            let place = self.parse_expression()?;
            if !place.is_place() {
                return Err(self.report(errors::invalid_mutable_argument(place.span)));
            }
            let span = first.span.to(place.span);
            return Ok(Argument {
                kind: ArgumentKind::Mutable(place),
                span,
            });
        }
        if first.kind == TokenKind::Identifier && self.peek().kind == TokenKind::Assign {
            self.advance();
            self.advance();
            let value = self.parse_expression()?;
            let span = first.span.to(value.span);
            return Ok(Argument {
                kind: ArgumentKind::Named {
                    name: self.name(first),
                    value,
                },
                span,
            });
        }
        let value = self.parse_expression()?;
        Ok(Argument {
            span: value.span,
            kind: ArgumentKind::Positional(value),
        })
    }

    fn parse_index(&mut self, object: Expr<'src>) -> ParseResult<Expr<'src>> {
        let open = self.advance();
        let index = self.parse_expression()?;
        let close = self.expect_closing(open, TokenKind::RightBracket, "`]`")?;
        let span = object.span.to(close.span);
        Ok(Expr {
            kind: ExprKind::Index {
                object: Box::new(object),
                index: Box::new(index),
            },
            span,
        })
    }

    fn parse_field_access(&mut self, object: Expr<'src>) -> ParseResult<Expr<'src>> {
        self.advance();
        if self.at(TokenKind::Dot) {
            return Err(self.reject_range(&object));
        }
        let field = self.expect_name("a field name after `.`")?;
        let span = object.span.to(field.span);
        Ok(Expr {
            kind: ExprKind::Field {
                object: Box::new(object),
                field,
            },
            span,
        })
    }

    /// `start..end` is a range in other languages. Lymra calls `range(start, end)`.
    fn reject_range(&mut self, start: &Expr<'src>) -> ErrorReported {
        self.advance();
        let end = match self.parse_binary(ADDITIVE_PRECEDENCE) {
            Ok(end) => end,
            Err(reported) => return reported,
        };
        let rewritten = format!("range({}, {})", self.text(start.span), self.text(end.span));
        self.report(errors::foreign_range(start.span.to(end.span), rewritten))
    }

    fn parse_primary(&mut self) -> ParseResult<Expr<'src>> {
        let token = self.current();
        let kind = match token.kind {
            TokenKind::Integer(value) => ExprKind::Integer(value),
            TokenKind::Float(value) => ExprKind::Float(value),
            TokenKind::Keyword(Keyword::True) => ExprKind::Bool(true),
            TokenKind::Keyword(Keyword::False) => ExprKind::Bool(false),
            TokenKind::Identifier => ExprKind::Name(self.text(token.span)),
            TokenKind::TextStart => return self.parse_text(),
            TokenKind::LeftParen => return self.parse_parenthesized(),
            _ => {
                let help = Help::text("write a value here: a name, a number, text, or a call");
                return Err(self.unexpected("an expression", help));
            }
        };
        self.advance();
        Ok(Expr {
            kind,
            span: token.span,
        })
    }

    /// Parses `( expression )`. The result is the inner expression, with a span that includes the parentheses.
    fn parse_parenthesized(&mut self) -> ParseResult<Expr<'src>> {
        let open = self.advance();
        if self.at(TokenKind::RightParen) {
            let close = self.advance();
            return Err(self.report(errors::empty_parentheses(open.span.to(close.span))));
        }
        let inner = self.parse_expression()?;
        let close = self.expect_closing(open, TokenKind::RightParen, "`)`")?;
        Ok(Expr {
            kind: inner.kind,
            span: open.span.to(close.span),
        })
    }

    /// Parses a text literal from its balanced run of tokens.
    fn parse_text(&mut self) -> ParseResult<Expr<'src>> {
        let start = self.advance();
        let mut parts = Vec::new();
        loop {
            let token = self.current();
            match token.kind {
                TokenKind::TextSegment => {
                    self.advance();
                    parts.push(TextPart::Literal(unescape(self.text(token.span))));
                }
                TokenKind::InterpolationStart => {
                    if let Some(interpolated) = self.parse_interpolation()? {
                        parts.push(TextPart::Interpolation(interpolated));
                    }
                }
                TokenKind::TextEnd => {
                    self.advance();
                    return Ok(Expr {
                        kind: ExprKind::Text(parts),
                        span: start.span.to(token.span),
                    });
                }
                _ => {
                    return Err(self.unexpected(
                        "the end of the text",
                        Help::text("close the text with `\"`"),
                    ));
                }
            }
        }
    }

    /// Parses `{ expression }` inside text. An empty `{}` is reported and yields nothing.
    fn parse_interpolation(&mut self) -> ParseResult<Option<Expr<'src>>> {
        let open = self.advance();
        if self.at(TokenKind::InterpolationEnd) {
            let close = self.advance();
            self.record(errors::empty_interpolation(open.span.to(close.span)));
            return Ok(None);
        }
        let interpolated = self.parse_expression()?;
        let closer = format!("`{}`", describe::closer(TokenKind::InterpolationEnd));
        self.expect_closing(open, TokenKind::InterpolationEnd, &closer)?;
        Ok(Some(interpolated))
    }
}
