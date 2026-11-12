/*!
 * Statements, blocks and control flow.
 *
 * Besides checking each statement, this tracks whether control can fall
 * through a statement to the one after it. That finds code that can never
 * run, and functions that can reach their end without returning a value.
 */

use super::arrays::place_parts;
use super::expressions::{CheckedCall, immutable_local_error};
use super::features::Feature;
use super::scope::{FunctionScope, LocalKind};
use super::{CheckResult, Checker, DISCARD, errors, evaluation_order};
use crate::ast::{self, ArgumentKind, Mutability};
use crate::diagnostics::{ErrorReported, Help};
use crate::ir::{self, IntegerType, LocalId, Type};
use crate::lexer::Keyword;
use crate::similarity;
use crate::source::Span;

/// What a `for` loop goes over.
enum Iteration {
    Range {
        start: ir::Expr,
        end: ir::Expr,
        ty: IntegerType,
    },
    /// The elements of the array that `array` computes.
    Elements {
        array: ir::Expr,
        element: Type,
        length: u64,
    },
}

/// A checked statement, and whether control can continue to the statement after it.
struct Checked {
    /// `None` when the statement was rejected.
    statement: Option<ir::Statement>,
    falls_through: bool,
}

impl Checked {
    fn continuing(statement: CheckResult<ir::Statement>) -> Self {
        Self {
            statement: statement.ok(),
            falls_through: true,
        }
    }
}

fn keyword_span(start: u32, keyword: Keyword) -> Span {
    Span::new(start, start + keyword.as_str().len() as u32)
}

impl<'src, 'ast> Checker<'src, 'ast> {
    /// Checks a block in its own scope. Returns the block and whether control can fall out of its end.
    pub(super) fn check_block(&mut self, block: &ast::Block<'src>) -> (ir::Block, bool) {
        if let Some(scope) = self.scope.as_mut() {
            scope.open_block();
        }
        let mut statements = Vec::new();
        let mut falls_through = true;
        let mut exit: Option<Span> = None;
        let mut unreachable_reported = false;
        for statement in &block.statements {
            if let (Some(exit), false) = (exit, unreachable_reported) {
                self.report(errors::unreachable(statement.span, exit));
                unreachable_reported = true;
            }
            let checked = self.check_statement(statement);
            statements.extend(checked.statement);
            if falls_through && !checked.falls_through {
                falls_through = false;
                exit = Some(statement.span);
            }
        }
        let closed = self
            .scope
            .as_mut()
            .map(FunctionScope::close_block)
            .unwrap_or_default();
        self.report_scope_exit(&closed);
        (ir::Block { statements }, falls_through)
    }

    fn check_statement(&mut self, statement: &ast::Statement<'src>) -> Checked {
        self.check_evaluation_order(statement);
        let span = statement.span;
        match &statement.kind {
            ast::StatementKind::Binding(binding) => {
                Checked::continuing(self.check_binding(binding, span))
            }
            ast::StatementKind::Assignment { target, value } => {
                Checked::continuing(self.check_assignment(target, value))
            }
            ast::StatementKind::Expression(expr) => {
                Checked::continuing(self.check_expression_statement(expr, span))
            }
            ast::StatementKind::If(statement) => self.check_if(statement),
            ast::StatementKind::While { condition, body } => self.check_while(condition, body),
            ast::StatementKind::For {
                variable,
                iterable,
                body,
            } => self.check_for(*variable, iterable, body),
            ast::StatementKind::Match { scrutinee, .. } => {
                self.mark_reads(scrutinee);
                let header = Span::new(span.start, scrutinee.span.end);
                self.report(Feature::Choices.diagnostic(header));
                Checked::continuing(Err(ErrorReported))
            }
            ast::StatementKind::Return(value) => Checked {
                statement: self.check_return(value.as_ref(), span).ok(),
                falls_through: false,
            },
            ast::StatementKind::Break => self.jump(Keyword::Break, span),
            ast::StatementKind::Continue => self.jump(Keyword::Continue, span),
            ast::StatementKind::Assert(condition) => Checked::continuing(
                self.check_condition(condition)
                    .map(|condition| ir::Statement::Assert { condition }),
            ),
        }
    }

    /// Checks each expression the statement evaluates on its own, since the conditions of an `if` run one at a time.
    fn check_evaluation_order(&mut self, statement: &ast::Statement<'src>) {
        match &statement.kind {
            ast::StatementKind::Binding(binding) => self.report_order_conflict(&[&binding.value]),
            ast::StatementKind::Assignment { target, value } => {
                // The target's own binding is where the value goes, not a use; its indexes are uses.
                let (_, steps) = place_parts(target);
                let mut parts: Vec<&ast::Expr<'src>> =
                    steps.iter().map(|step| step.index).collect();
                parts.push(value);
                self.report_order_conflict(&parts);
            }
            ast::StatementKind::Expression(expr)
            | ast::StatementKind::Assert(expr)
            | ast::StatementKind::Return(Some(expr)) => self.report_order_conflict(&[expr]),
            ast::StatementKind::If(statement) => {
                for branch in &statement.branches {
                    self.report_order_conflict(&[&branch.condition]);
                }
            }
            ast::StatementKind::While { condition, .. } => {
                self.report_order_conflict(&[condition]);
            }
            ast::StatementKind::For { iterable, .. } => self.report_order_conflict(&[iterable]),
            ast::StatementKind::Match { .. }
            | ast::StatementKind::Return(None)
            | ast::StatementKind::Break
            | ast::StatementKind::Continue => {}
        }
    }

    fn report_order_conflict(&mut self, parts: &[&ast::Expr<'src>]) {
        if let Some(conflict) = evaluation_order::find_conflict(parts) {
            self.report(errors::changed_and_used(&conflict));
        }
    }

    fn check_binding(
        &mut self,
        binding: &ast::Binding<'src>,
        span: Span,
    ) -> CheckResult<ir::Statement> {
        let declared = binding.ty.as_ref().map(|ty| self.resolve_type(ty));
        let value = self.check_against(&binding.value, declared.as_ref().map(Result::as_ref));
        let keyword = match binding.mutability {
            Mutability::Constant => keyword_span(span.start, Keyword::Constant),
            Mutability::Variable => keyword_span(span.start, Keyword::Variable),
        };

        if binding.name.text == DISCARD {
            if binding.mutability == Mutability::Variable {
                self.report(errors::discard_variable(keyword));
                return Err(ErrorReported);
            }
            return value.map(ir::Statement::Discard);
        }

        let ty = match (declared, &value) {
            (Some(declared), _) => declared,
            (None, Ok(value)) => Ok(value.ty.clone()),
            (None, Err(error)) => Err(*error),
        };
        let kind = match binding.mutability {
            Mutability::Constant => LocalKind::Constant,
            Mutability::Variable => LocalKind::Variable,
        };
        let local = self.declare_local(binding.name, ty, kind, Some(keyword), value.is_err())?;
        Ok(ir::Statement::Declare {
            local,
            value: value?,
        })
    }

    /// Checks `target = value`, where the target is a binding or an element inside one.
    fn check_assignment(
        &mut self,
        target: &ast::Expr<'src>,
        value: &ast::Expr<'src>,
    ) -> CheckResult<ir::Statement> {
        let (root, steps) = place_parts(target);
        let ast::ExprKind::Name(name) = &root.kind else {
            self.check_for_errors(value);
            return Err(self.report_unassignable(root));
        };
        let name = *name;
        if name == DISCARD {
            self.check_for_errors(value);
            return Err(self.report(errors::discard_assigned(root.span)));
        }

        let Some(id) = self.scope.as_ref().and_then(|scope| scope.lookup(name)) else {
            self.check_for_errors(value);
            return Err(self.report_assignment_to_unknown(name, root.span));
        };
        let Some(local) = self.scope.as_mut().and_then(|scope| scope.local_mut(id)) else {
            return Err(ErrorReported);
        };
        if !local.kind.is_mutable() {
            // The binding is clearly in use; only the change is wrong.
            local.read = true;
            let error = immutable_local_error(local, root.span);
            self.check_for_errors(value);
            return Err(self.report(error));
        }
        local.written = true;
        let place = local
            .ty
            .clone()
            .and_then(|ty| self.check_place_indexes(ty, &steps));
        match place {
            Ok((indexes, ty)) => {
                let value = self.check_expr(value, Some(&ty))?;
                Ok(ir::Statement::Assign {
                    target: ir::Place { local: id, indexes },
                    value,
                })
            }
            Err(ErrorReported) => {
                self.check_untyped(value);
                Err(ErrorReported)
            }
        }
    }

    /// Reports an assignment target that does not start at a binding, such as `make()[0]`.
    fn report_unassignable(&mut self, root: &ast::Expr<'src>) -> ErrorReported {
        if let ast::ExprKind::Field { object, .. } = &root.kind {
            return self.reject_field(object, root.span);
        }
        self.mark_reads(root);
        self.report(errors::element_of_value(root.span))
    }

    /// An assignment to a name that is not a local: undeclared, a top-level constant, or a function.
    fn report_assignment_to_unknown(&mut self, name: &'src str, span: Span) -> ErrorReported {
        if self.globals.contains_key(name) || super::BuiltIn::of(name).is_some() {
            return self.report_change_to_global(name, span);
        }
        let candidates: Vec<&str> = self
            .scope
            .iter()
            .flat_map(|scope| scope.visible_names())
            .collect();
        let suggestion = similarity::closest_match(name, candidates);
        // The suggested variable was most likely the one meant, so it counts as changed.
        if let Some(intended) = suggestion {
            if let Some(local) = self.scope.as_mut().and_then(|scope| {
                let id = scope.lookup(intended)?;
                scope.local_mut(id)
            }) {
                local.written = true;
            }
        }
        self.report(errors::undeclared_assignment(name, span, suggestion))
    }

    fn check_expression_statement(
        &mut self,
        expr: &ast::Expr<'src>,
        span: Span,
    ) -> CheckResult<ir::Statement> {
        if let ast::ExprKind::Call(call) = &expr.kind {
            return match self.check_call(call, expr.span, None)? {
                CheckedCall::Effect { statement, .. } => Ok(statement),
                CheckedCall::Value(_) => {
                    let function = self.text(call.callee.span);
                    Err(self.report(errors::unused_result(function, span)))
                }
            };
        }
        self.check_expr_with_hint(expr, None)?;
        let help = match &expr.kind {
            ast::ExprKind::Binary {
                operator: ast::BinaryOperator::Equal,
                left,
                right,
            } if matches!(left.kind, ast::ExprKind::Name(_)) => {
                let assignment = format!("{} = {}", self.text(left.span), self.text(right.span));
                Help::replace(
                    "to change the value, assign with a single `=`",
                    expr.span,
                    assignment,
                )
            }
            _ => Help::text(
                "remove it, or use the value: store it in a binding or pass it to a call",
            ),
        };
        Err(self.report(errors::unused_value(expr.span, help)))
    }

    fn check_if(&mut self, statement: &ast::IfStatement<'src>) -> Checked {
        let mut branches = Vec::new();
        let mut failed = false;
        let mut falls_through = statement.otherwise.is_none();
        for branch in &statement.branches {
            let condition = self.check_condition(&branch.condition);
            let (body, body_falls_through) = self.check_block(&branch.body);
            falls_through |= body_falls_through;
            match condition {
                Ok(condition) => branches.push(ir::Branch { condition, body }),
                Err(ErrorReported) => failed = true,
            }
        }
        let otherwise = statement.otherwise.as_ref().map(|block| {
            let (body, body_falls_through) = self.check_block(block);
            falls_through |= body_falls_through;
            body
        });
        Checked {
            statement: (!failed).then_some(ir::Statement::If {
                branches,
                otherwise,
            }),
            falls_through,
        }
    }

    fn check_while(&mut self, condition: &ast::Expr<'src>, body: &ast::Block<'src>) -> Checked {
        let checked_condition = self.check_condition(condition);
        let runs_forever = matches!(condition.kind, ast::ExprKind::Bool(true));
        let (body, has_break) = self.check_loop_body(body);
        Checked {
            statement: checked_condition
                .ok()
                .map(|condition| ir::Statement::While { condition, body }),
            falls_through: !runs_forever || has_break,
        }
    }

    /// Checks a loop body. Returns the body and whether a `break` leaves the loop.
    fn check_loop_body(&mut self, body: &ast::Block<'src>) -> (ir::Block, bool) {
        if let Some(scope) = self.scope.as_mut() {
            scope.enter_loop();
        }
        let (body, _) = self.check_block(body);
        let has_break = self.scope.as_mut().is_some_and(FunctionScope::exit_loop);
        (body, has_break)
    }

    fn check_for(
        &mut self,
        variable: ast::Name<'src>,
        iterable: &ast::Expr<'src>,
        body: &ast::Block<'src>,
    ) -> Checked {
        let iteration = self.check_iterable(iterable);
        if let Some(scope) = self.scope.as_mut() {
            scope.open_block();
        }
        let local: CheckResult<Option<LocalId>> = if variable.text == DISCARD {
            Ok(None)
        } else {
            let ty = match &iteration {
                Ok(Iteration::Range { ty, .. }) => Ok(Type::Integer(*ty)),
                Ok(Iteration::Elements { element, .. }) => Ok(element.clone()),
                Err(error) => Err(*error),
            };
            self.declare_local(variable, ty, LocalKind::LoopVariable, None, false)
                .map(Some)
        };
        let (body, _) = self.check_loop_body(body);
        let closed = self
            .scope
            .as_mut()
            .map(FunctionScope::close_block)
            .unwrap_or_default();
        self.report_scope_exit(&closed);

        let statement = match (iteration, local) {
            (Ok(Iteration::Range { start, end, .. }), Ok(variable)) => {
                Some(ir::Statement::ForRange {
                    variable,
                    start,
                    end,
                    body,
                })
            }
            (Ok(Iteration::Elements { array, length, .. }), Ok(variable)) => {
                Some(ir::Statement::ForEach {
                    variable,
                    array,
                    length,
                    body,
                })
            }
            _ => None,
        };
        // A range can be empty, so control always reaches the statement after the loop.
        Checked {
            statement,
            falls_through: true,
        }
    }

    /// Checks what a `for` loop goes over: `range(start, end)`, or the elements of an array.
    fn check_iterable(&mut self, iterable: &ast::Expr<'src>) -> CheckResult<Iteration> {
        if let ast::ExprKind::Call(call) = &iterable.kind {
            if matches!(call.callee.kind, ast::ExprKind::Name("range")) {
                return self.check_range(call, iterable.span);
            }
        }
        let checked = self.check_expr_with_hint(iterable, None)?;
        match &checked.ty {
            Type::Array(array) => Ok(Iteration::Elements {
                element: array.element.clone(),
                length: array.length,
                array: checked,
            }),
            Type::String => Err(self.report(Feature::ForEach.diagnostic(iterable.span))),
            Type::Integer(_) | Type::Boolean => {
                let source = self.text(iterable.span);
                Err(self.report(errors::not_iterable(&checked.ty, iterable.span, source)))
            }
        }
    }

    /// Checks `range(start, end)`, which only a `for` loop can go over.
    fn check_range(&mut self, call: &ast::Call<'src>, span: Span) -> CheckResult<Iteration> {
        if let Some(block) = &call.block {
            self.mark_argument_reads(&call.arguments);
            let with = Span::new(block.span.start, block.span.start + "with".len() as u32);
            return Err(self.report(Feature::CodeArguments.diagnostic(with)));
        }
        let arguments = Span::new(call.callee.span.end, span.end);
        let [start, end] = call.arguments.as_slice() else {
            self.check_arguments_for_errors(&call.arguments);
            return Err(self.report(errors::range_argument_count(
                call.arguments.len(),
                arguments,
            )));
        };
        let (ArgumentKind::Positional(start), ArgumentKind::Positional(end)) =
            (&start.kind, &end.kind)
        else {
            self.check_arguments_for_errors(&call.arguments);
            return Err(self.report(errors::range_arguments_plain(arguments)));
        };
        let (start, end) = self.check_operands(start, end)?;
        let ty = self.shared_integer_type("range", &start, &end)?;
        Ok(Iteration::Range { start, end, ty })
    }

    fn check_return(
        &mut self,
        value: Option<&ast::Expr<'src>>,
        span: Span,
    ) -> CheckResult<ir::Statement> {
        let Some(scope) = self.scope.as_ref() else {
            return Err(ErrorReported);
        };
        let function = scope.function_name.text;
        let keyword = keyword_span(span.start, Keyword::Return);
        match (value, scope.return_type.clone()) {
            (Some(value), Ok(Some(ty))) => {
                let value = self.check_expr(value, Some(&ty))?;
                Ok(ir::Statement::Return(Some(value)))
            }
            (Some(value), Ok(None)) => {
                self.check_for_errors(value);
                let removal = Span::new(keyword.end, value.span.end);
                Err(self.report(errors::return_value_without_type(
                    function, value.span, removal,
                )))
            }
            (None, Ok(Some(ty))) => {
                Err(self.report(errors::return_without_value(function, &ty, keyword)))
            }
            (None, Ok(None)) => Ok(ir::Statement::Return(None)),
            (value, Err(ErrorReported)) => {
                if let Some(value) = value {
                    self.check_untyped(value);
                }
                Err(ErrorReported)
            }
        }
    }

    /// A `break` or `continue`. One outside a loop is rejected, and then it does not hide the code after it.
    fn jump(&mut self, keyword: Keyword, span: Span) -> Checked {
        let statement = self.check_jump(keyword, span).ok();
        Checked {
            falls_through: statement.is_none(),
            statement,
        }
    }

    fn check_jump(&mut self, keyword: Keyword, span: Span) -> CheckResult<ir::Statement> {
        let Some(scope) = self.scope.as_mut() else {
            return Err(ErrorReported);
        };
        if !scope.in_loop() {
            return Err(self.report(errors::jump_outside_loop(keyword.as_str(), span)));
        }
        if keyword == Keyword::Break {
            scope.record_break();
            return Ok(ir::Statement::Break);
        }
        Ok(ir::Statement::Continue)
    }

    /// Checks the condition of an `if`, `while` or `assert`, which has to be a `boolean`.
    fn check_condition(&mut self, condition: &ast::Expr<'src>) -> CheckResult<ir::Expr> {
        let checked = self.check_expr_with_hint(condition, None)?;
        if checked.ty != Type::Boolean {
            let source = self.text(condition.span);
            return Err(self.report(errors::condition_not_boolean(
                &checked.ty,
                condition.span,
                source,
            )));
        }
        Ok(checked)
    }

    /// Reports the bindings of a closed scope that were never read, or declared `variable` and never changed.
    pub(super) fn report_scope_exit(&mut self, closed: &[LocalId]) {
        let Some(scope) = self.scope.as_ref() else {
            return;
        };
        let mut found = Vec::new();
        for local in closed.iter().filter_map(|id| scope.local(*id)) {
            if local.ty.is_err() || local.declaration_failed || local.name.text == DISCARD {
                continue;
            }
            let name = local.name;
            let noun = local.kind.noun();
            // A `variable` parameter that is only assigned hands its value to the caller.
            let passed_back = matches!(local.kind, LocalKind::VariableParameter) && local.written;
            if !local.read && !passed_back {
                let help = match local.kind {
                    LocalKind::Parameter | LocalKind::VariableParameter => Help::replace(
                        "if the function has to take this parameter, name it `_`",
                        name.span,
                        "_",
                    ),
                    LocalKind::LoopVariable => Help::replace(
                        "if the loop only repeats, name the variable `_`",
                        name.span,
                        "_",
                    ),
                    LocalKind::Constant | LocalKind::Variable => {
                        Help::text("use it, or remove its declaration")
                    }
                };
                if local.written {
                    found.push(errors::assigned_but_never_read(name.text, name.span));
                } else {
                    found.push(errors::unused(name.text, noun, name.span, help));
                }
            } else if local.kind.is_mutable() && !local.written {
                let fix = match (local.kind, local.keyword) {
                    (LocalKind::VariableParameter, Some(keyword)) => Help::remove(
                        "remove `variable`; the function only reads this parameter",
                        keyword,
                    ),
                    (_, Some(keyword)) => {
                        Help::replace("declare it with `constant`", keyword, "constant")
                    }
                    (_, None) => Help::text("declare it with `constant`"),
                };
                found.push(errors::never_changed(name.text, noun, name.span, fix));
            }
        }
        self.diagnostics.extend(found);
    }
}
