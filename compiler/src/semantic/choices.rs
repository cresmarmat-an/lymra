/*!
 * Types the program declares with `choice`: a value that is exactly one of
 * its variants (spec 4.7). A variant is built like a declared type, by
 * naming its fields, or by its name alone when it holds none. A `match`
 * takes a value apart again: a `case` for every variant, each naming the
 * fields it uses, and none for a variant twice (`L0401`).
 */

use std::sync::Arc;

use super::errors::{self, Stranger};
use super::expressions::CheckedCall;
use super::scope::{FunctionScope, LocalKind};
use super::{CheckResult, Checker, DISCARD, Global, TypeId};
use crate::ast;
use crate::diagnostics::ErrorReported;
use crate::ir::{self, ChoiceType, CustomType, Type};
use crate::lint::case::Case;
use crate::similarity;
use crate::source::Span;

impl<'src, 'ast> Checker<'src, 'ast> {
    /// Resolves the variants of a `choice`, each with its fields. Each is checked, even after one fails.
    pub(super) fn resolve_choice(
        &mut self,
        declaration: &'ast ast::ChoiceDeclaration<'src>,
    ) -> CheckResult<Type> {
        let mut variants = Vec::with_capacity(declaration.variants.len());
        let mut failed = false;
        for variant in &declaration.variants {
            match self.resolve_fields(variant.name.text, &variant.fields) {
                Ok(variant) => variants.push(variant),
                Err(ErrorReported) => failed = true,
            }
        }
        if failed {
            return Err(ErrorReported);
        }
        Ok(Type::Choice(Arc::new(ChoiceType {
            name: declaration.name.text.to_owned(),
            variants,
        })))
    }

    /// The choice declared as the type `id`, which declares a variant named at `reference`.
    fn choice_of_variant(&mut self, id: TypeId, reference: Span) -> CheckResult<Arc<ChoiceType>> {
        match self.declared_type(id, reference)? {
            Type::Choice(choice) => Ok(choice),
            // A variant always belongs to a choice; anything else was reported where it was declared.
            Type::Integer(_)
            | Type::Float(_)
            | Type::Boolean
            | Type::String
            | Type::Array(_)
            | Type::Custom(_)
            | Type::List(_) => Err(ErrorReported),
        }
    }

    /// A variant named on its own, as in `Dot`: a value of its choice, if the variant holds no fields.
    pub(super) fn check_variant_name(
        &mut self,
        choice: TypeId,
        variant: usize,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        let choice = self.choice_of_variant(choice, span)?;
        let Some(declared) = choice.variants.get(variant) else {
            return Err(ErrorReported);
        };
        if !declared.fields.is_empty() {
            let missing: Vec<&str> = declared
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect();
            return Err(self.report(errors::missing_fields(&declared.name, &missing, span)));
        }
        Ok(ir::Expr {
            kind: ir::ExprKind::Variant {
                variant,
                fields: Vec::new(),
            },
            ty: Type::Choice(choice),
            span,
        })
    }

    /// A variant built with its fields, as in `Circle(radius = 2)`.
    pub(super) fn check_variant_call(
        &mut self,
        choice: TypeId,
        variant: usize,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let choice = match self.choice_of_variant(choice, call.callee.span) {
            Ok(choice) => choice,
            Err(error) => {
                self.check_arguments_for_errors(&call.arguments);
                return Err(error);
            }
        };
        let Some(declared) = choice.variants.get(variant) else {
            return Err(ErrorReported);
        };
        if declared.fields.is_empty() {
            self.check_arguments_for_errors(&call.arguments);
            let brackets = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::fieldless_variant_called(&declared.name, brackets)));
        }
        let fields = self.check_field_values(declared, &call.arguments, span)?;
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Variant { variant, fields },
            ty: Type::Choice(Arc::clone(&choice)),
            span,
        }))
    }

    /**
     * Checks `match value` and its cases. Returns the statement, and whether
     * control can continue after it: when some case can reach its end. A
     * case left out is an error already, so it does not count.
     */
    pub(super) fn check_match(
        &mut self,
        value: &ast::Expr<'src>,
        arms: &[ast::MatchArm<'src>],
        header: Span,
    ) -> (CheckResult<ir::Statement>, bool) {
        let value = self.check_expr_with_hint(value, None);
        let choice = match &value {
            Ok(checked) => match &checked.ty {
                Type::Choice(choice) => Some(Arc::clone(choice)),
                other => {
                    let error = errors::match_needs_choice(other, checked.span);
                    self.report(error);
                    None
                }
            },
            Err(ErrorReported) => None,
        };
        let mut handled: Vec<Option<Span>> =
            vec![None; choice.as_ref().map_or(0, |choice| choice.variants.len())];
        let mut cases = Vec::with_capacity(arms.len());
        let mut failed = choice.is_none();
        let mut falls_through = arms.is_empty();
        for arm in arms {
            let variant = choice
                .as_deref()
                .and_then(|choice| self.case_variant(choice, &arm.pattern, &mut handled));
            let (case, arm_falls_through) = self.check_case(choice.as_deref(), variant, arm);
            falls_through |= arm_falls_through;
            match case {
                Some(case) => cases.push(case),
                None => failed = true,
            }
        }
        if let Some(choice) = &choice {
            let missing: Vec<&CustomType> = choice
                .variants
                .iter()
                .zip(&handled)
                .filter(|(_, handled)| handled.is_none())
                .map(|(variant, _)| variant)
                .collect();
            if !missing.is_empty() {
                self.report(errors::missing_cases(&choice.name, &missing, header));
                failed = true;
            }
        }
        let statement = match value {
            Ok(value) if !failed => Ok(ir::Statement::Match { value, cases }),
            Ok(_) | Err(ErrorReported) => Err(ErrorReported),
        };
        (statement, falls_through)
    }

    /// The position of the variant a `case` names in `choice`, unless it names something else or a variant handled already.
    fn case_variant(
        &mut self,
        choice: &ChoiceType,
        pattern: &ast::Pattern<'src>,
        handled: &mut [Option<Span>],
    ) -> Option<usize> {
        let name = pattern.variant;
        let Some((index, _)) = choice.variant(name.text) else {
            let stranger = self.stranger(choice, name.text);
            let error = errors::not_a_variant(&choice.name, name.text, name.span, stranger);
            self.report(error);
            return None;
        };
        if let Some(previous) = handled.get(index).copied().flatten() {
            let error = errors::variant_handled_twice(name.text, pattern.span, previous);
            self.report(error);
            return None;
        }
        if let Some(slot) = handled.get_mut(index) {
            *slot = Some(pattern.span);
        }
        Some(index)
    }

    /// What a `case` names when it is not a variant of `choice`.
    fn stranger<'name>(&self, choice: &'name ChoiceType, name: &str) -> Stranger<'name>
    where
        'src: 'name,
    {
        if name == DISCARD {
            return Stranger::Everything;
        }
        if let Some(Global::Variant { choice: other, .. }) = self.globals.get(name).copied() {
            if let Some(entry) = self.types.get(other) {
                return Stranger::Elsewhere {
                    choice: entry.declaration.name().text,
                };
            }
        }
        Stranger::Unknown {
            suggestion: similarity::closest_match(
                name,
                choice.variants.iter().map(|variant| variant.name.as_str()),
            ),
        }
    }

    /**
     * Checks one `case`: its bindings, each a field of the variant, then its
     * body, in a scope that holds them. Where the variant is unknown, the
     * bindings are declared without a type, so the body is checked without
     * the same mistake reported again.
     */
    fn check_case(
        &mut self,
        choice: Option<&ChoiceType>,
        variant: Option<usize>,
        arm: &ast::MatchArm<'src>,
    ) -> (Option<ir::Case>, bool) {
        let declared = choice
            .zip(variant)
            .and_then(|(choice, variant)| choice.variants.get(variant));
        if let Some(scope) = self.scope.as_mut() {
            scope.open_block();
        }
        let mut bindings = Vec::with_capacity(arm.pattern.bindings.len());
        let mut failed = declared.is_none();
        for binding in &arm.pattern.bindings {
            let written = binding.field;
            let field = declared.map(|declared| (declared, declared.field(written.text)));
            let (ty, field) = match field {
                Some((_, Some((position, field)))) => (Ok(field.ty.clone()), Some(position)),
                Some((declared, None)) => {
                    let suggestion = similarity::closest_match(
                        written.text,
                        declared.fields.iter().map(|field| field.name.as_str()),
                    );
                    let error = errors::unknown_field(
                        &declared.name,
                        written.text,
                        written.span,
                        suggestion,
                    );
                    (Err(self.report(error)), None)
                }
                None => (Err(ErrorReported), None),
            };
            let renamed = binding.name.text != written.text;
            if let (Some(variant), false, Ok(_)) = (declared, renamed, &ty) {
                if let Some(previous) = self.visible_declaration(written.text) {
                    let suggested =
                        format!("{}_{}", Case::Snake.convert(&variant.name), written.text);
                    let error =
                        errors::field_name_taken(written.text, written.span, previous, &suggested);
                    self.report(error);
                    failed = true;
                    continue;
                }
            }
            let local = self.declare_local(binding.name, ty, LocalKind::CaseBinding, None, false);
            match (local, field) {
                (Ok(local), Some(field)) => bindings.push(ir::CaseBinding { local, field }),
                (Ok(_) | Err(ErrorReported), _) => failed = true,
            }
        }
        let (body, falls_through) = self.check_block(&arm.body);
        let closed = self
            .scope
            .as_mut()
            .map(FunctionScope::close_block)
            .unwrap_or_default();
        self.report_scope_exit(&closed);
        let case = match variant {
            Some(variant) if !failed => Some(ir::Case {
                variant,
                bindings,
                body,
            }),
            Some(_) | None => None,
        };
        (case, falls_through)
    }
}
