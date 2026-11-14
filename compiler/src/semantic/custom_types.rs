/*!
 * Types the program declares with `type`: resolving their fields, building
 * a value with every field named, and reading a field. A declared type is
 * resolved when something first names it (see `cycles`); a `choice` is
 * resolved here too, from the fields of its variants (see `choices`).
 */

use std::sync::Arc;

use super::expressions::CheckedCall;
use super::{CheckResult, Checker, DeclaredType, Definition, TypeId, TypeState, errors};
use crate::ast::{self, ArgumentKind};
use crate::diagnostics::ErrorReported;
use crate::ir::{self, CustomType, FieldType, Type};
use crate::similarity;
use crate::source::Span;

impl<'src, 'ast> Checker<'src, 'ast> {
    /// The declared type `id`, resolving it first if needed. `reference` is where it is named.
    pub(super) fn declared_type(&mut self, id: TypeId, reference: Span) -> CheckResult<Type> {
        let Some(entry) = self.types.get(id) else {
            return Err(ErrorReported);
        };
        let declaration = entry.declaration;
        match &entry.state {
            TypeState::Done(ty) => return Ok(ty.clone()),
            TypeState::Failed => return Err(ErrorReported),
            TypeState::InProgress => {
                let diagnostic = self.cycle(Definition::Type(id), reference);
                return Err(self.report(diagnostic));
            }
            TypeState::Pending => {}
        }
        self.set_type_state(id, TypeState::InProgress);
        let resolved = self.defining(Definition::Type(id), reference, |checker| {
            let ty = match declaration {
                DeclaredType::Record(declaration) => {
                    let custom =
                        checker.resolve_fields(declaration.name.text, &declaration.fields)?;
                    Type::Custom(Arc::new(custom))
                }
                DeclaredType::Choice(declaration) => checker.resolve_choice(declaration)?,
            };
            checker.fits_on_stack(ty, declaration.name().span)
        });
        let state = match &resolved {
            Ok(ty) => TypeState::Done(ty.clone()),
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

    /**
     * Resolves the fields of a type or a variant called `name`. A field name
     * used twice is error `L0207`. Each field is checked, even after one fails.
     */
    pub(super) fn resolve_fields(
        &mut self,
        name: &str,
        fields: &'ast [ast::Field<'src>],
    ) -> CheckResult<CustomType> {
        let mut resolved = Vec::with_capacity(fields.len());
        let mut failed = false;
        for (position, field) in fields.iter().enumerate() {
            let earlier = fields[..position]
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
                Ok(ty) => resolved.push(FieldType {
                    name: field.name.text.to_owned(),
                    ty,
                }),
                Err(ErrorReported) => failed = true,
            }
        }
        if failed {
            return Err(ErrorReported);
        }
        Ok(CustomType {
            name: name.to_owned(),
            fields: resolved,
        })
    }

    /// Checks a construction such as `Frog(name = "Kero", age = 3)`.
    pub(super) fn check_construction(
        &mut self,
        id: TypeId,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let custom = match self.declared_type(id, call.callee.span) {
            Ok(Type::Custom(custom)) => custom,
            Ok(Type::Choice(choice)) => {
                self.check_arguments_for_errors(&call.arguments);
                return Err(self.report(errors::choice_called(&choice, call.callee.span)));
            }
            Ok(_) | Err(ErrorReported) => {
                self.check_arguments_for_errors(&call.arguments);
                return Err(ErrorReported);
            }
        };
        let values = self.check_field_values(&custom, &call.arguments, span)?;
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::Construct(values),
            ty: Type::Custom(custom),
            span,
        }))
    }

    /**
     * Checks the values given for the fields of `record`, a type or a
     * variant: every field once, by name, in any order. The values run in
     * the order they are written. `span` is the whole construction.
     */
    pub(super) fn check_field_values(
        &mut self,
        record: &CustomType,
        arguments: &[ast::Argument<'src>],
        span: Span,
    ) -> CheckResult<Vec<ir::FieldValue>> {
        let mut given: Vec<Option<Span>> = vec![None; record.fields.len()];
        let mut values = Vec::with_capacity(arguments.len());
        let mut failed = false;
        // A misnamed field is likely the one that seems missing, so a missing one is only reported without them.
        let mut misnamed = false;
        for (position, argument) in arguments.iter().enumerate() {
            let (name, value) = match &argument.kind {
                ArgumentKind::Named { name, value } => (name, value),
                ArgumentKind::Positional(value) => {
                    self.check_for_errors(value);
                    let field = record.fields.get(position).map(|field| field.name.as_str());
                    let written = self.text(value.span);
                    self.report(errors::unnamed_field(
                        &record.name,
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
            let Some((index, field)) = record.field(name.text) else {
                self.check_for_errors(value);
                let suggestion = similarity::closest_match(
                    name.text,
                    record.fields.iter().map(|field| field.name.as_str()),
                );
                self.report(errors::unknown_field(
                    &record.name,
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
        let missing: Vec<&str> = record
            .fields
            .iter()
            .zip(&given)
            .filter(|(_, given)| given.is_none())
            .map(|(field, _)| field.name.as_str())
            .collect();
        if !missing.is_empty() && !misnamed {
            self.report(errors::missing_fields(&record.name, &missing, span));
            failed = true;
        }
        if failed {
            return Err(ErrorReported);
        }
        Ok(values)
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
