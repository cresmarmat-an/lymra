//! Types as written in source: `integer64`, `list[Item]`, `array[float32, 4]`, `packed Shape`.

use super::{ParseResult, Parser, errors};
use crate::ast::{Name, Path, TypeArgument, TypeExpr, TypeExprKind};
use crate::foreign_types::translate_type;
use crate::lexer::{Keyword, TokenKind};

impl<'src> Parser<'src, '_> {
    pub(super) fn parse_type(&mut self) -> ParseResult<TypeExpr<'src>> {
        self.nested(Self::parse_type_at_depth)
    }

    fn parse_type_at_depth(&mut self) -> ParseResult<TypeExpr<'src>> {
        if self.at_keyword(Keyword::Packed) {
            let keyword = self.advance();
            let inner = self.parse_type()?;
            let span = keyword.span.to(inner.span);
            return Ok(TypeExpr {
                kind: TypeExprKind::Packed(Box::new(inner)),
                span,
            });
        }

        let path = self.parse_path("a type")?;
        if let [name] = path.segments.as_slice() {
            if let Some(translation) = translate_type(name.text) {
                self.record(errors::foreign_type(name.span, name.text, translation));
            }
        }
        let mut span = path.span;
        let mut arguments = Vec::new();
        if self.at(TokenKind::LeftBracket) {
            let open = self.advance();
            let (parsed, close) =
                self.parse_list(open, TokenKind::RightBracket, Self::parse_type_argument)?;
            if parsed.is_empty() {
                return Err(self.report(errors::empty_type_arguments(open.span.to(close.span))));
            }
            arguments = parsed;
            span = span.to(close.span);
        }
        Ok(TypeExpr {
            kind: TypeExprKind::Named { path, arguments },
            span,
        })
    }

    fn parse_type_argument(&mut self) -> ParseResult<TypeArgument<'src>> {
        let token = self.current();
        if let TokenKind::Integer(value) = token.kind {
            self.advance();
            return Ok(TypeArgument::Size {
                value,
                span: token.span,
            });
        }
        self.parse_type().map(TypeArgument::Type)
    }

    /// Parses names joined by dots. `expected` says what the first name is, for the error message.
    pub(super) fn parse_path(&mut self, expected: &str) -> ParseResult<Path<'src>> {
        let first = self.expect_name(expected)?;
        let mut span = first.span;
        let mut segments = vec![first];
        while self.eat(TokenKind::Dot) {
            let segment = self.expect_name("a name after `.`")?;
            span = span.to(segment.span);
            segments.push(segment);
        }
        Ok(Path { segments, span })
    }

    /// Parses `type name`, as in a parameter or a field. `role` names what is being declared.
    pub(super) fn parse_typed_name(
        &mut self,
        role: &str,
    ) -> ParseResult<(TypeExpr<'src>, Name<'src>)> {
        if self.at(TokenKind::Identifier) && self.peek().kind == TokenKind::Colon {
            return self.parse_name_colon_type();
        }
        let ty = self.parse_type()?;
        // A foreign type name has been reported as a type already; it is not a name without one.
        let untyped_name = lone_name(&ty).filter(|name| translate_type(name.text).is_none());
        if let Some(name) = untyped_name {
            // One bare word followed by the end of the entry is a name whose type was left out.
            let ends_entry = matches!(
                self.current().kind,
                TokenKind::Comma
                    | TokenKind::RightParen
                    | TokenKind::Newline
                    | TokenKind::EndOfFile
            );
            if ends_entry {
                return Err(self.report(errors::missing_type(role, name.text, name.span)));
            }
        }
        let name = self.expect_name(&format!("a {role} name"))?;
        Ok((ty, name))
    }

    /// Reads `name: type`, the order other languages use, and reports the Lymra order.
    pub(super) fn parse_name_colon_type(&mut self) -> ParseResult<(TypeExpr<'src>, Name<'src>)> {
        let name_token = self.advance();
        self.advance();
        let ty = self.parse_type()?;
        let name = self.name(name_token);
        self.record(errors::type_after_name(
            name.span.to(ty.span),
            name.text,
            self.text(ty.span),
        ));
        Ok((ty, name))
    }
}

/// The name of a type written as a single word with no arguments.
fn lone_name<'src>(ty: &TypeExpr<'src>) -> Option<Name<'src>> {
    match &ty.kind {
        TypeExprKind::Named { path, arguments }
            if arguments.is_empty() && path.segments.len() == 1 =>
        {
            path.segments.first().copied()
        }
        _ => None,
    }
}
