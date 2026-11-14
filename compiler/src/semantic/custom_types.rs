/*!
 * Types the program declares with `type`: resolving their fields, building
 * a value with every field named, and reading a field. A type is resolved
 * when something first names it (see `cycles`).
 */

use std::sync::Arc;

use super::arrays::MAX_VALUE_SIZE;
use super::expressions::CheckedCall;
use super::{CheckResult, Checker, Definition, TypeId, TypeState, errors};
use crate::ast::{self, ArgumentKind};
use crate::diagnostics::ErrorReported;
use crate::ir::{self, CustomType, FieldType, Type};
use crate::similarity;
use crate::source::Span;

impl<'src, 'ast> Checker<'src, 'ast> {
    /// The declared type `id`, resolving its fields first if needed. `reference` is where it is named.
    pub(super) fn custom_type(
        &mut self,
        id: TypeId,
        reference: Span,
    ) -> CheckResult<Arc<CustomType>> {
        let Some(entry) = self.types.get(id) else {
            return Err(ErrorReported);
        };
        let declaration = entry.declaration;
        match &entry.state {
            TypeState::Done(ty) => return Ok(Arc::clone(ty)),
            TypeState::Failed => return Err(ErrorReported),
            TypeState::InProgress => {
                let diagnostic = self.cycle(Definition::Type(id), reference);
                return Err(self.report(diagnostic));
            }
            TypeState::Pending => {}
        }
        self.set_type_state(id, TypeState::InProgress);
        let resolved = self.defining(Definition::Type(id), reference, |checker| {
            checker.resolve_fields(declaration)
        });
        let state = match &resolved {
            Ok(ty) => TypeState::Done(Arc::clone(ty)),
            Err(ErrorReported) => TypeState::Failed,
        };
        self.set_type_state(id, state);
        resolved
    }

    fn set_type_state(&mut self, id: TypeId, state: TypeState) {
        if let Some(entry) = self.types.get_mut(id) {
            entry.state = state;
        }
    }

    /// Resolves every field of a declaration. Each field is checked, even after one fails.
    fn resolve_fields(
        &mut self,
        declaration: &'ast ast::TypeDeclaration<'src>,
    ) -> CheckResult<Arc<CustomType>> {
        let mut fields = Vec::with_capacity(declaration.fields.len());
        let mut failed = false;
        for (position, field) in declaration.fields.iter().enumerate() {
            let earlier = declaration.fields[..position]
                .iter()
                .find(|other| other.name.text == field.name.text);
            if let Some(earlier) = earlier {
                self.report(errors::already_declared(
                    field.name.text,
                    field.name.span,
                    earlier.name.span,
                ));
                failed = true;
                continue;
            }
            match self.resolve_type(&field.ty) {
                Ok(ty) => fields.push(FieldType {
                    name: field.name.text.to_owned(),
                    ty,
                }),
                Err(ErrorReported) => failed = true,
            }
        }
        if failed {
            return Err(ErrorReported);
        }
        let custom = Arc::new(CustomType {
            name: declaration.name.text.to_owned(),
            fields,
        });
        let ty = Type::Custom(Arc::clone(&custom));
        match ty.size() {
            Some(size) if size <= MAX_VALUE_SIZE => Ok(custom),
            size => Err(self.report(errors::too_large_for_stack(
                &ty,
                size,
                declaration.name.span,
            ))),
        }
    }

    /**
     * Checks a construction such as `Frog(name = "Kero", age = 3)`: every
     * field given once, by name, in any order. The values run in the order
     * they are written.
     */
    pub(super) fn check_construction(
        &mut self,
        id: TypeId,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let custom = match self.custom_type(id, call.callee.span) {
            Ok(custom) => custom,
            Err(error) => {
                self.check_arguments_for_errors(&call.arguments);
                return Err(error);
            }
        };
        let mut given: Vec<Option<Span>> = vec![None; custom.fields.len()];
        let mut values = Vec::with_capacity(call.arguments.len());
        let mut failed = false;
        // A misnamed field is likely the one that seems missing, so a missing one is only reported without them.
        let mut misnamed = false;
        for (position, argument) in call.arguments.iter().enumerate() {
            let (name, value) = match &argument.kind {
                ArgumentKind::Named { name, value } => (name, value),
                ArgumentKind::Positional(value) => {
                    self.check_for_errors(value);
                    let field = custom.fields.get(position).map(|field| field.name.as_str());
                    let written = self.text(value.span);
                    self.report(errors::unnamed_field(
                        &custom.name,
                        field,
                        argument.span,
                        written,
                    ));
                    (failed, misnamed) = (true, true);
                    continue;
                }
                ArgumentKind::Mutable(value) => {
                    self.check_for_errors(value);
                    let mark = Span::new(argument.span.start, value.span.start);
                    self.report(errors::needless_variable_mark("field", mark));
                    failed = true;
                    continue;
                }
            };
            let Some((index, field)) = custom.field(name.text) else {
                self.check_for_errors(value);
                let suggestion = similarity::closest_match(
                    name.text,
                    custom.fields.iter().map(|field| field.name.as_str()),
                );
                self.report(errors::unknown_field(
                    &custom.name,
                    name.text,
                    name.span,
                    suggestion,
                ));
                (failed, misnamed) = (true, true);
                continue;
            };
            if let Some(previous) = given[index] {
                self.check_for_errors(value);
                self.report(errors::field_given_twice(
                    name.text,
                    argument.span,
                    previous,
                ));
                failed = true;
                continue;
            }
            given[index] = Some(argument.span);
            match self.check_expr(value, Some(&field.ty)) {
                Ok(value) => values.push(ir::FieldValue {
                    field: index,
                    value,
                }),
                Err(ErrorReported) => failed = true,
            }
        }
        let missing: Vec<&str> = custom
            .fields
            .iter()
            .zip(&given)
            .filter(|(_, given)| given.is_none())
            .map(|(field, _)| field.name.as_str())
            .collect();
        if !missing.is_empty() && !misnamed {
            self.report(errors::missing_fields(&custom.name, &missing, span));
            failed = true;
        }
        if failed {
            return Err(ErrorReported);
        }
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Construct(values),
            ty: Type::Custom(custom),
            span,
        }))
    }

    /// Checks `object.field`, where `object` is a value.
    pub(super) fn check_field(
        &mut self,
        object: &ast::Expr<'src>,
        field: ast::Name<'src>,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        let object = self.check_expr_with_hint(object, None)?;
        let (index, ty) = self.field_of(&object.ty, field, object.span)?;
        Ok(ir::Expr {
            kind: ir::ExprKind::Field {
                object: Box::new(object),
                field: index,
            },
            ty,
            span,
        })
    }

    /// The position and the type of `field` in a value of type `ty`, written at `object`.
    pub(super) fn field_of(
        &mut self,
        ty: &Type,
        field: ast::Name<'src>,
        object: Span,
    ) -> CheckResult<(usize, Type)> {
        let Type::Custom(custom) = ty else {
            return Err(self.report(errors::no_fields(ty, object, field.text)));
        };
        if let Some((index, found)) = custom.field(field.text) {
            return Ok((index, found.ty.clone()));
        }
        let suggestion = similarity::closest_match(
            field.text,
            custom.fields.iter().map(|field| field.name.as_str()),
        );
        Err(self.report(errors::unknown_field(
            &custom.name,
            field.text,
            field.span,
            suggestion,
        )))
    }
}
