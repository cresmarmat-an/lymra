/*!
 * Functions that can fail (spec 8): what `fails` may name, `fail`, `try`,
 * and the values a call to such a function gives. A call gives a result, a
 * choice the compiler makes with the variants `Success` and `Failure`, so a
 * `return` gives a `Success`, a `fail` gives a `Failure`, and `match` takes
 * a result apart like any other choice.
 */

use super::expressions::CheckedCall;
use super::features::Feature;
use super::statements::keyword_span;
use super::{CheckResult, Checker, errors};
use crate::ast;
use crate::diagnostics::ErrorReported;
use crate::ir::{self, ChoiceType, FAILURE_VARIANT, SUCCESS_VARIANT, Type};
use crate::lexer::Keyword;
use crate::source::Span;

/// `Success` of the result type `result`, which holds `value` unless the function returns nothing.
pub(super) fn success(value: Option<ir::Expr>, result: &Type, span: Span) -> ir::Expr {
    let fields = value
        .map(|value| ir::FieldValue { field: 0, value })
        .into_iter()
        .collect();
    ir::Expr {
        kind: ir::ExprKind::Variant {
            variant: SUCCESS_VARIANT,
            fields,
        },
        ty: result.clone(),
        span,
    }
}

/// `Failure` of the result type `result`, which holds `error`.
fn failure(error: ir::Expr, result: &Type) -> ir::Expr {
    let span = error.span;
    ir::Expr {
        kind: ir::ExprKind::Variant {
            variant: FAILURE_VARIANT,
            fields: vec![ir::FieldValue {
                field: 0,
                value: error,
            }],
        },
        ty: result.clone(),
        span,
    }
}

/// What a `Success` of a call's result holds, or `None` when its function returns nothing.
fn success_type(result: &ir::Expr) -> Option<Type> {
    result
        .ty
        .as_result()
        .and_then(ChoiceType::success_type)
        .cloned()
}

impl<'src> Checker<'src, '_> {
    /**
     * The type after `fails` in `declaration`, if it has one. A failure
     * holds nothing that grows, no view and no handle (L0802), and in 0.1
     * the value a function that can fail returns holds no list or view
     * either, since its result holds that value.
     */
    pub(super) fn resolve_failure(
        &mut self,
        declaration: &ast::Function<'src>,
        returns: &CheckResult<Option<Type>>,
    ) -> CheckResult<Option<Type>> {
        let Some(written) = &declaration.fails else {
            return Ok(None);
        };
        let error = self.resolve_type(written)?;
        if error.is_growable() || error.holds_view() || error.holds_handle() {
            return Err(self.report(errors::failure_type_not_allowed(&error, written.span)));
        }
        if let (Ok(Some(value)), Some(declared)) = (returns, &declaration.return_type) {
            if value.is_growable() || value.holds_view() {
                return Err(self.report(Feature::FailingGrowableOrView.diagnostic(declared.span)));
            }
        }
        Ok(Some(error))
    }

    /// What `return` gives: `value` itself, or a `Success` holding it in a function that can fail.
    pub(super) fn returned(&self, value: Option<ir::Expr>, span: Span) -> Option<ir::Expr> {
        match self.scope.as_ref().and_then(|scope| scope.result.as_ref()) {
            Some(result) => Some(success(value, result, span)),
            None => value,
        }
    }

    /// `fail value`, which ends the function with a `Failure` holding the value.
    pub(super) fn check_fail(
        &mut self,
        value: &ast::Expr<'src>,
        span: Span,
    ) -> CheckResult<ir::Statement> {
        let Some(scope) = self.scope.as_ref() else {
            return Err(ErrorReported);
        };
        let function = scope.function_name.text;
        match (scope.fails.clone(), scope.result.clone()) {
            (Ok(Some(error)), Some(result)) => {
                let value = self.check_expr(value, Some(&error))?;
                Ok(ir::Statement::Return(Some(failure(value, &result))))
            }
            (Ok(None), _) => {
                let value = self.check_expr(value, None)?;
                let keyword = keyword_span(span.start, Keyword::Fail);
                Err(self.report(errors::fail_without_fails(function, &value.ty, keyword)))
            }
            (Ok(Some(_)) | Err(ErrorReported), _) => {
                self.check_for_errors(value);
                Err(ErrorReported)
            }
        }
    }

    /**
     * `try call`, or `try call else fail value`, as a value: what the call
     * returns on a `Success`. `else fail` may only stand as the whole value
     * of a statement (L0805).
     */
    pub(super) fn check_try(
        &mut self,
        attempt: &ast::Expr<'src>,
        fallback: Option<&ast::Expr<'src>>,
        span: Span,
    ) -> CheckResult<ir::Expr> {
        if fallback.is_some() && self.whole_value != Some(span) {
            self.check_for_errors(attempt);
            fallback
                .into_iter()
                .for_each(|value| self.check_for_errors(value));
            return Err(self.report(errors::fallback_inside_expression(span)));
        }
        let checked = self.check_attempt(attempt, fallback, span)?;
        match success_type(&checked.result) {
            Some(ty) => Ok(ir::Expr {
                kind: ir::ExprKind::Try(Box::new(checked)),
                ty,
                span,
            }),
            None => Err(self.report(errors::no_value(self.callee_text(attempt), span))),
        }
    }

    /// `try call` as a statement of its own, for a function that returns nothing; a value it returns would be lost (L0505).
    pub(super) fn check_try_statement(
        &mut self,
        attempt: &ast::Expr<'src>,
        fallback: Option<&ast::Expr<'src>>,
        span: Span,
    ) -> CheckResult<ir::Statement> {
        let checked = self.check_attempt(attempt, fallback, span)?;
        if success_type(&checked.result).is_some() {
            return Err(self.report(errors::unused_result(self.callee_text(attempt), span)));
        }
        Ok(ir::Statement::Try(checked))
    }

    /// The function an attempted call names, as written.
    fn callee_text(&self, attempt: &ast::Expr<'src>) -> &'src str {
        match &attempt.kind {
            ast::ExprKind::Call(call) => self.text(call.callee.span),
            _ => self.text(attempt.span),
        }
    }

    /**
     * The parts of a `try`: a call to a function that can fail (L0803), and
     * the value to fail with in place of its error, if one is given. The
     * function that holds the `try` has to fail with that value's type, or
     * else with the call's own (L0801).
     */
    fn check_attempt(
        &mut self,
        attempt: &ast::Expr<'src>,
        fallback: Option<&ast::Expr<'src>>,
        span: Span,
    ) -> CheckResult<ir::Attempt> {
        let result = match self.check_attempted_call(attempt) {
            Ok(result) => result,
            Err(ErrorReported) => {
                fallback
                    .into_iter()
                    .for_each(|value| self.check_for_errors(value));
                return Err(ErrorReported);
            }
        };
        let passed = result
            .ty
            .as_result()
            .and_then(ChoiceType::failure_type)
            .cloned()
            .ok_or(ErrorReported)?;
        let Some(scope) = self.scope.as_ref() else {
            return Err(ErrorReported);
        };
        let function = scope.function_name.text;
        let returns = scope.result.clone();
        let keyword = keyword_span(span.start, Keyword::Try);
        let declared = match scope.fails.clone() {
            Ok(Some(declared)) => declared,
            Ok(None) => {
                let shown = match fallback {
                    Some(value) => self.check_expr(value, None)?.ty,
                    None => passed,
                };
                return Err(self.report(errors::try_without_fails(function, &shown, keyword)));
            }
            Err(ErrorReported) => {
                fallback
                    .into_iter()
                    .for_each(|value| self.check_for_errors(value));
                return Err(ErrorReported);
            }
        };
        let fallback = match fallback {
            Some(value) => Some(self.check_expr(value, Some(&declared))?),
            None if passed == declared => None,
            None => {
                return Err(self.report(errors::try_wrong_failure(
                    function, &passed, &declared, keyword,
                )));
            }
        };
        Ok(ir::Attempt {
            result,
            fallback,
            returns: returns.ok_or(ErrorReported)?,
        })
    }

    /// The call after `try`, which has to be a call to a function that can fail (L0803).
    fn check_attempted_call(&mut self, attempt: &ast::Expr<'src>) -> CheckResult<ir::Expr> {
        let ast::ExprKind::Call(call) = &attempt.kind else {
            self.check_for_errors(attempt);
            return Err(self.report(errors::try_needs_failing_call(
                "this is not a call",
                attempt.span,
            )));
        };
        match self.check_call(call, attempt.span, None)? {
            CheckedCall::Value(result) if result.ty.as_result().is_some() => Ok(result),
            CheckedCall::Value(_) | CheckedCall::Effect { .. } => {
                let cannot = format!("`{}` cannot fail", self.text(call.callee.span));
                Err(self.report(errors::try_needs_failing_call(&cannot, attempt.span)))
            }
        }
    }
}
