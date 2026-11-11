//! Top-level declarations: `use`, `constant`, `function`, `type` and `choice`.

use super::{ParseResult, Parser, errors, foreign};
use crate::ast::{
    ChoiceDeclaration, Field, Function, Item, ItemKind, Mutability, Parameter, Path,
    TypeDeclaration, TypeExpr, Variant, Visibility,
};
use crate::diagnostics::{ErrorReported, Help};
use crate::lexer::{Keyword, Token, TokenKind};
use crate::source::Span;

/// A parsed declaration together with the source it covers.
type ParsedItem<'src> = ParseResult<(ItemKind<'src>, Span)>;

impl<'src> Parser<'src, '_> {
    pub(super) fn parse_item(&mut self) -> ParseResult<Item<'src>> {
        let start = self.current().span;
        let visibility = self.parse_visibility()?;
        let (kind, span) = self.parse_item_kind()?;
        Ok(Item {
            visibility,
            kind,
            span: start.to(span),
        })
    }

    fn parse_visibility(&mut self) -> ParseResult<Visibility> {
        let token = self.current();
        let word = self.text(token.span);
        let is_foreign = token.kind == TokenKind::Identifier
            && foreign::is_visibility(word)
            && matches!(
                self.peek().kind,
                TokenKind::Keyword(_) | TokenKind::Identifier
            );
        if is_foreign {
            self.record(errors::foreign_keyword(
                token.span,
                word,
                Keyword::Public.as_str(),
            ));
        } else if !self.at_keyword(Keyword::Public) {
            return Ok(Visibility::Private);
        }
        self.advance();

        let declared = self.current();
        let declares = match declared.kind {
            TokenKind::Keyword(
                Keyword::Function | Keyword::Type | Keyword::Choice | Keyword::Constant,
            ) => true,
            // A foreign spelling such as `struct` is reported when the declaration itself is parsed.
            TokenKind::Identifier => {
                let word = self.text(declared.span);
                foreign::item_keyword(word).is_some_and(|keyword| keyword != Keyword::Use)
                    || foreign::binding_keyword(word) == Some(Mutability::Constant)
            }
            _ => false,
        };
        if declares {
            return Ok(Visibility::Public);
        }
        let help = Help::text(
            "`public` goes before `function`, `type`, `choice` or `constant`; remove it here",
        );
        Err(self.unexpected("a declaration after `public`", help))
    }

    fn parse_item_kind(&mut self) -> ParsedItem<'src> {
        let token = self.current();
        match token.kind {
            TokenKind::Keyword(keyword) => self.parse_keyword_item(keyword),
            TokenKind::Identifier => self.parse_identifier_item(token),
            TokenKind::Directive => Err(self.report(errors::late_directive(token.span))),
            _ => Err(self.report_statement_at_top_level()),
        }
    }

    /// Parses the declaration `keyword` starts. The current token is that keyword or a foreign spelling of it.
    fn parse_keyword_item(&mut self, keyword: Keyword) -> ParsedItem<'src> {
        let span = self.current().span;
        let declares = matches!(
            keyword,
            Keyword::Use | Keyword::Function | Keyword::Type | Keyword::Choice
        );
        if declares {
            self.body_not_indented = false;
        }
        match keyword {
            Keyword::Use => self.parse_use(),
            Keyword::Constant => self.parse_constant_item(),
            Keyword::Function => self.parse_function(),
            Keyword::Type => self.parse_type_declaration(),
            Keyword::Choice => self.parse_choice(),
            Keyword::Variable => Err(self.report(errors::variable_at_top_level(span))),
            Keyword::External => {
                Err(self.report(errors::unavailable_feature(span, "`external` declarations")))
            }
            _ => Err(self.report_statement_at_top_level()),
        }
    }

    /// A line that starts with a plain word: a keyword from another language, a typo, or a stray statement.
    fn parse_identifier_item(&mut self, token: Token) -> ParsedItem<'src> {
        let word = self.text(token.span);
        // A declaration keyword is followed by a name. A word followed by anything else is not a declaration.
        if matches!(
            self.peek().kind,
            TokenKind::Identifier | TokenKind::Keyword(_)
        ) {
            if let Some(keyword) = foreign::item_keyword(word) {
                // `external` is not available yet, so its foreign spelling gets that one error and no other.
                if keyword != Keyword::External {
                    self.record(errors::foreign_keyword(token.span, word, keyword.as_str()));
                }
                return self.parse_keyword_item(keyword);
            }
            match foreign::binding_keyword(word) {
                Some(Mutability::Constant) => {
                    self.record(errors::foreign_binding_keyword(
                        token.span,
                        word,
                        Keyword::Constant.as_str(),
                    ));
                    return self.parse_constant_item();
                }
                Some(Mutability::Variable) => {
                    return Err(self.report(errors::variable_at_top_level(token.span)));
                }
                None => {}
            }
            if let Some(keyword) = foreign::misspelled(word, &foreign::ITEM_KEYWORDS) {
                return Err(self.report(errors::misspelled_keyword(
                    token.span,
                    word,
                    keyword.as_str(),
                )));
            }
        }
        Err(self.report_statement_at_top_level())
    }

    fn report_statement_at_top_level(&mut self) -> ErrorReported {
        // Already explained: by the lexer, or as the body of a declaration that was not indented.
        if self.at(TokenKind::Error) || self.body_not_indented {
            return ErrorReported;
        }
        let line = self.rest_of_line();
        self.report(errors::statement_at_top_level(line))
    }

    fn parse_use(&mut self) -> ParsedItem<'src> {
        let keyword = self.advance();
        let path = self.parse_path("a module name")?;
        let follows = self.current();
        if follows.kind == TokenKind::Identifier && self.text(follows.span) == "from" {
            return Err(self.reject_use_from(keyword, &path));
        }
        self.end_statement()?;
        let span = keyword.span.to(path.span);
        Ok((ItemKind::Use(path), span))
    }

    /// `use item from module` is the import form the spec removed under goal 4.
    fn reject_use_from(&mut self, keyword: Token, item: &Path<'src>) -> ErrorReported {
        self.advance();
        let module = self.rest_of_line();
        if module.is_empty() {
            return self.unexpected(
                "a module name",
                Help::text("write an import as one path: `use module.item`"),
            );
        }
        let rewritten = format!("use {}.{}", self.text(module), self.text(item.span));
        self.report(errors::foreign_use_from(keyword.span.to(module), rewritten))
    }

    fn parse_constant_item(&mut self) -> ParsedItem<'src> {
        let (binding, end, span) = self.parse_binding(Mutability::Constant)?;
        self.finish_statement(end)?;
        Ok((ItemKind::Constant(binding), span))
    }

    fn parse_function(&mut self) -> ParsedItem<'src> {
        let keyword = self.advance();
        let name = self.expect_name("a function name")?;
        if self.at(TokenKind::LeftBracket) {
            let span = self.current().span;
            return Err(self.report(errors::unavailable_feature(span, "generic functions")));
        }
        let (parameters, closing_paren) = self.parse_parameters()?;
        let return_type = self.parse_return_type()?;
        let header_end = return_type.as_ref().map_or(closing_paren, |ty| ty.span);
        let body = self.parse_block(keyword.span.to(header_end))?;
        let span = keyword.span.to(body.span);
        Ok((
            ItemKind::Function(Function {
                name,
                parameters,
                return_type,
                body,
            }),
            span,
        ))
    }

    fn parse_parameters(&mut self) -> ParseResult<(Vec<Parameter<'src>>, Span)> {
        if !self.at(TokenKind::LeftParen) {
            let help =
                Help::text("write the parameters in parentheses after the name; `()` means none");
            return Err(self.unexpected("`(`", help));
        }
        let open = self.advance();
        let (parameters, close) =
            self.parse_list(open, TokenKind::RightParen, Self::parse_parameter)?;
        Ok((parameters, close.span))
    }

    fn parse_parameter(&mut self) -> ParseResult<Parameter<'src>> {
        let start = self.current().span;
        let mutability = if self.eat_keyword(Keyword::Variable) {
            Mutability::Variable
        } else {
            Mutability::Constant
        };
        let (ty, name) = self.parse_typed_name("parameter")?;
        Ok(Parameter {
            mutability,
            ty,
            name,
            span: start.to(name.span),
        })
    }

    fn parse_return_type(&mut self) -> ParseResult<Option<TypeExpr<'src>>> {
        let token = self.current();
        match token.kind {
            TokenKind::Keyword(Keyword::Returns) => {}
            TokenKind::Arrow => self.record(errors::foreign_return_type(token.span, "->")),
            // `: type` is a return type in other languages. A `:` that ends the line is handled with the block.
            TokenKind::Colon if self.peek().kind != TokenKind::Newline => {
                self.record(errors::foreign_return_type(token.span, ":"));
            }
            TokenKind::Keyword(Keyword::Return) => {
                self.record(errors::misspelled_keyword(
                    token.span,
                    "return",
                    Keyword::Returns.as_str(),
                ));
            }
            _ => return Ok(None),
        }
        self.advance();
        self.parse_type().map(Some)
    }

    fn parse_type_declaration(&mut self) -> ParsedItem<'src> {
        let keyword = self.advance();
        let name = self.expect_name("a type name")?;
        let header = keyword.span.to(name.span);
        let fields = self.parse_indented_lines(header, Self::parse_field_line)?;
        let span = fields.last().map_or(header, |field| header.to(field.span));
        Ok((ItemKind::Type(TypeDeclaration { name, fields }), span))
    }

    fn parse_field_line(&mut self) -> ParseResult<Field<'src>> {
        let field = self.parse_field()?;
        self.end_statement()?;
        Ok(field)
    }

    fn parse_field(&mut self) -> ParseResult<Field<'src>> {
        let start = self.current().span;
        let (ty, name) = self.parse_typed_name("field")?;
        Ok(Field {
            ty,
            name,
            span: start.to(name.span),
        })
    }

    fn parse_choice(&mut self) -> ParsedItem<'src> {
        let keyword = self.advance();
        let name = self.expect_name("a choice name")?;
        let header = keyword.span.to(name.span);
        let variants = self.parse_indented_lines(header, Self::parse_variant_line)?;
        let span = variants
            .last()
            .map_or(header, |variant| header.to(variant.span));
        Ok((ItemKind::Choice(ChoiceDeclaration { name, variants }), span))
    }

    fn parse_variant_line(&mut self) -> ParseResult<Variant<'src>> {
        let name = self.expect_name("a variant name")?;
        let mut span = name.span;
        let mut fields = Vec::new();
        if self.at(TokenKind::LeftParen) {
            let open = self.advance();
            let (parsed, close) =
                self.parse_list(open, TokenKind::RightParen, Self::parse_field)?;
            if parsed.is_empty() {
                self.record(errors::empty_variant_fields(open.span.to(close.span)));
            }
            fields = parsed;
            span = span.to(close.span);
        }
        self.end_statement()?;
        Ok(Variant { name, fields, span })
    }
}
