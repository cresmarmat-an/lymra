//! Directives: the `#!` lines at the top of a file (spec 2.3).

use super::{ParseResult, Parser, errors};
use crate::ast::{Directive, DirectiveKind, Edition, OptimizeMode};
use crate::diagnostics::{Help, Label};
use crate::lexer::TokenKind;
use crate::similarity;
use crate::source::Span;

const DIRECTIVE_NAMES: [&str; 3] = ["edition", "optimize", "prove"];
const OPTIMIZE_MODES: [&str; 3] = ["speed", "size", "debug"];

impl Parser<'_, '_> {
    /// Parses the directive lines that open the file. Rejected ones are reported and left out.
    pub(super) fn parse_directives(&mut self) -> Vec<Directive> {
        let mut directives = Vec::new();
        while self.at(TokenKind::Directive) {
            let line = self.advance().span;
            self.eat(TokenKind::Newline);
            if let Ok(directive) = self.interpret_directive(line, &directives) {
                directives.push(directive);
            }
        }
        directives
    }

    fn interpret_directive(&mut self, line: Span, earlier: &[Directive]) -> ParseResult<Directive> {
        let text = self.text(line);
        let mut words = text.get(2..).unwrap_or_default().split_whitespace();
        let name = words.next().unwrap_or_default();
        let arguments: Vec<&str> = words.collect();

        let kind = match name {
            "edition" => self.edition_directive(line, &arguments)?,
            "optimize" => self.optimize_directive(line, &arguments)?,
            "prove" => self.prove_directive(line, &arguments)?,
            "nocheck" | "nonstrict" => {
                return Err(self.report(errors::safety_switch_directive(name, line)));
            }
            "strict" => return Err(self.report(errors::strict_directive(line))),
            _ => {
                let name_start = line.start + text.find(name).unwrap_or_default() as u32;
                let name_span = if name.is_empty() {
                    line
                } else {
                    Span::new(name_start, name_start + name.len() as u32)
                };
                let closest = similarity::closest_match(name, DIRECTIVE_NAMES);
                return Err(self.report(errors::unknown_directive(name, name_span, closest)));
            }
        };

        let same_kind = |directive: &&Directive| {
            std::mem::discriminant(&directive.kind) == std::mem::discriminant(&kind)
        };
        if let Some(first) = earlier.iter().find(same_kind) {
            return Err(self.report(errors::duplicate_directive(name, line, first.span)));
        }
        Ok(Directive { kind, span: line })
    }

    fn edition_directive(&mut self, line: Span, arguments: &[&str]) -> ParseResult<DirectiveKind> {
        let fix = |message: &str| Help::replace(message, line, "#!edition 0.1");
        match arguments {
            ["0.1"] => Ok(DirectiveKind::Edition(Edition::V0_1)),
            [] => Err(self.report(errors::directive_misuse(
                "`#!edition` needs a version".to_owned(),
                Label::new(line, "no version given"),
                fix("name the edition this file is written for"),
            ))),
            [version] => Err(self.report(errors::directive_misuse(
                format!("edition `{version}` is not supported"),
                Label::new(line, "this compiler implements edition 0.1"),
                fix("use edition 0.1"),
            ))),
            _ => Err(self.report(errors::directive_misuse(
                "`#!edition` takes exactly one version".to_owned(),
                Label::new(line, "more than one word after `#!edition`"),
                fix("give a single version"),
            ))),
        }
    }

    fn optimize_directive(&mut self, line: Span, arguments: &[&str]) -> ParseResult<DirectiveKind> {
        let modes = "write `#!optimize speed`, `#!optimize size` or `#!optimize debug`";
        match arguments {
            ["speed"] => Ok(DirectiveKind::Optimize(OptimizeMode::Speed)),
            ["size"] => Ok(DirectiveKind::Optimize(OptimizeMode::Size)),
            ["debug"] => Ok(DirectiveKind::Optimize(OptimizeMode::Debug)),
            [mode] => {
                let help = match similarity::closest_match(mode, OPTIMIZE_MODES) {
                    Some(closest) => Help::replace(
                        format!("did you mean `{closest}`?"),
                        line,
                        format!("#!optimize {closest}"),
                    ),
                    None => Help::text(modes),
                };
                Err(self.report(errors::directive_misuse(
                    format!("`{mode}` is not an optimization mode"),
                    Label::new(line, "the modes are `speed`, `size` and `debug`"),
                    help,
                )))
            }
            _ => Err(self.report(errors::directive_misuse(
                "`#!optimize` takes exactly one mode".to_owned(),
                Label::new(line, "expected one word after `#!optimize`"),
                Help::text(modes),
            ))),
        }
    }

    fn prove_directive(&mut self, line: Span, arguments: &[&str]) -> ParseResult<DirectiveKind> {
        if arguments == ["bounds"] {
            return Ok(DirectiveKind::ProveBounds);
        }
        Err(self.report(errors::directive_misuse(
            "`#!prove` can only prove `bounds`".to_owned(),
            Label::new(line, "expected `#!prove bounds`"),
            Help::replace("write `#!prove bounds`", line, "#!prove bounds"),
        )))
    }
}
