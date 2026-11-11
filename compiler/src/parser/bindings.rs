//! `variable` and `constant` declarations (spec 3).

use super::statements::StatementEnd;
use super::{ParseResult, Parser, errors};
use crate::ast::{Binding, Mutability, Name, TypeExpr};
use crate::diagnostics::Help;
use crate::lexer::TokenKind;
use crate::source::Span;

const BINDING_NAME: &str = "a name for the binding";

impl<'src> Parser<'src, '_> {
    /**
     * Parses a declaration. The current token is its keyword, or a foreign
     * spelling of it that the caller has already reported, so `mutability`
     * is passed in instead of being read from the token.
     */
    pub(super) fn parse_binding(
        &mut self,
        mutability: Mutability,
    ) -> ParseResult<(Binding<'src>, StatementEnd, Span)> {
        let keyword = self.advance();
        let (ty, name) = self.parse_binding_target()?;
        match self.current().kind {
            TokenKind::Assign => {
                self.advance();
            }
            TokenKind::Newline | TokenKind::EndOfFile => {
                let declaration = keyword.span.to(name.span);
                return Err(self.report(errors::missing_initializer(name.text, declaration)));
            }
            _ => {
                return Err(self.unexpected(
                    "`=`",
                    Help::text("give the binding its value with `= <value>`"),
                ));
            }
        }
        let (value, end) = self.parse_value()?;
        let span = keyword.span.to(value.span);
        Ok((
            Binding {
                mutability,
                ty,
                name,
                value,
            },
            end,
            span,
        ))
    }

    /// Parses what follows the keyword: a name alone when the type is inferred, or a type and a name.
    fn parse_binding_target(&mut self) -> ParseResult<(Option<TypeExpr<'src>>, Name<'src>)> {
        let first = self.current();
        // A keyword right before `=` stands where the name belongs, not where a type does.
        if matches!(first.kind, TokenKind::Keyword(_)) && self.peek().kind == TokenKind::Assign {
            return self.expect_name(BINDING_NAME).map(|name| (None, name));
        }
        if first.kind == TokenKind::Identifier {
            match self.peek().kind {
                /*
                 * A single word before `=` is the name. The same word at the end
                 * of the line is a name with no value, which the caller reports.
                 */
                TokenKind::Assign | TokenKind::Newline | TokenKind::EndOfFile => {
                    self.advance();
                    return Ok((None, self.name(first)));
                }
                TokenKind::Colon => {
                    let (ty, name) = self.parse_name_colon_type()?;
                    return Ok((Some(ty), name));
                }
                _ => {}
            }
        }
        let ty = self.parse_type()?;
        let name = self.expect_name(BINDING_NAME)?;
        Ok((Some(ty), name))
    }
}
