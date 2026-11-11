/*!
 * The naming rule (spec 2.6): values, functions and modules are
 * `snake_case`, and types are `PascalCase`.
 *
 * Only declarations are checked. A use of a badly named thing is not a
 * second mistake, and fixing the declaration is what fixes the code.
 */

use super::case::Case;
use crate::ast::visit::{self, Visitor};
use crate::ast::{
    Expr, ExprKind, Field, Item, ItemKind, Module, Mutability, Name, Path, Statement, StatementKind,
};
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::lexer::Keyword;

/// The name that declares a binding nobody reads. It is exempt from the style rule.
const DISCARD: &str = "_";

pub fn check(module: &Module<'_>) -> Vec<Diagnostic> {
    let mut checker = NamingChecker {
        diagnostics: Vec::new(),
    };
    visit::walk_module(&mut checker, module);
    checker.diagnostics
}

struct NamingChecker {
    diagnostics: Vec<Diagnostic>,
}

impl NamingChecker {
    /// Checks one declared name. `kind` says what it declares, for the message.
    fn require(&mut self, name: Name<'_>, case: Case, kind: &str) {
        if !case.matches(name.text) {
            self.diagnostics.push(violation(name, case, kind));
        }
    }

    /// Checks the name of a binding, which may also be the discard name.
    fn require_binding(&mut self, name: Name<'_>, kind: &str) {
        if name.text != DISCARD {
            self.require(name, Case::Snake, kind);
        }
    }

    fn require_fields(&mut self, fields: &[Field<'_>]) {
        for field in fields {
            self.require(field.name, Case::Snake, "field");
        }
    }

    /// Module names are `snake_case`. The last name may instead be a type the path imports.
    fn check_use(&mut self, path: &Path<'_>) {
        let Some((last, modules)) = path.segments.split_last() else {
            return;
        };
        for module in modules {
            self.require(*module, Case::Snake, "module");
        }
        if !Case::Pascal.matches(last.text) {
            self.require(*last, Case::Snake, "imported");
        }
    }
}

impl<'src> Visitor<'src> for NamingChecker {
    fn visit_item(&mut self, item: &Item<'src>) {
        match &item.kind {
            ItemKind::Use(path) => self.check_use(path),
            ItemKind::Constant(binding) => self.require_binding(binding.name, "constant"),
            ItemKind::Function(function) => {
                self.require(function.name, Case::Snake, "function");
                for parameter in &function.parameters {
                    self.require_binding(parameter.name, "parameter");
                }
            }
            ItemKind::Type(declaration) => {
                self.require(declaration.name, Case::Pascal, "type");
                self.require_fields(&declaration.fields);
            }
            ItemKind::Choice(declaration) => {
                self.require(declaration.name, Case::Pascal, "choice");
                for variant in &declaration.variants {
                    self.require(variant.name, Case::Pascal, "variant");
                    self.require_fields(&variant.fields);
                }
            }
        }
        visit::walk_item(self, item);
    }

    fn visit_statement(&mut self, statement: &Statement<'src>) {
        match &statement.kind {
            StatementKind::Binding(binding) => {
                let kind = match binding.mutability {
                    Mutability::Constant => "constant",
                    Mutability::Variable => "variable",
                };
                self.require_binding(binding.name, kind);
            }
            StatementKind::For { variable, .. } => self.require_binding(*variable, "loop variable"),
            StatementKind::Match { arms, .. } => {
                for name in arms.iter().flat_map(|arm| &arm.pattern.bindings) {
                    self.require_binding(*name, "binding");
                }
            }
            _ => {}
        }
        visit::walk_statement(self, statement);
    }

    fn visit_expr(&mut self, expr: &Expr<'src>) {
        if let ExprKind::Call(call) = &expr.kind {
            for parameter in call.block.iter().flat_map(|block| &block.parameters) {
                self.require_binding(*parameter, "block parameter");
            }
        }
        visit::walk_expr(self, expr);
    }
}

fn violation(name: Name<'_>, case: Case, kind: &str) -> Diagnostic {
    let style = case.name();
    let corrected = case.convert(name.text);
    // A rewrite that is empty or a reserved word cannot be offered as the new name.
    let is_usable = !corrected.is_empty() && Keyword::from_text(&corrected).is_none();
    let (message, help) = if is_usable {
        (
            format!(
                "{kind} name `{}` should be written `{corrected}`",
                name.text
            ),
            Help::replace(
                "rename it here and everywhere it is used",
                name.span,
                corrected,
            ),
        )
    } else {
        (
            format!("{kind} name `{}` is not {style}", name.text),
            Help::text(format!("choose a {style} name")),
        )
    };
    Diagnostic::new(
        ErrorCode::L0103,
        message,
        Label::new(name.span, format!("{kind} names are {style}")),
        help,
    )
}
