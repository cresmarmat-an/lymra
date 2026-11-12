//! Top-level declarations, the types they name, and the constants computed from them.

use super::features::Feature;
use super::{
    BuiltIn, CheckResult, Checker, ConstantEntry, ConstantState, DISCARD, FunctionEntry, Global,
    Parameter, errors,
};
use crate::ast::{self, Mutability, Name, Visibility};
use crate::diagnostics::ErrorReported;
use crate::ir::{self, ConstantId, IntegerType, Type, Value};
use crate::similarity;
use crate::source::Span;

const ENTRY_POINT: &str = "main";

impl<'src, 'ast> Checker<'src, 'ast> {
    /**
     * Registers every top-level name first, then resolves the function
     * signatures, so declarations may appear in any order in the file.
     */
    pub(super) fn collect_items(&mut self, items: &'ast [ast::Item<'src>]) {
        for item in items {
            let public = item.visibility == Visibility::Public;
            match &item.kind {
                ast::ItemKind::Use(path) => {
                    self.report(Feature::Modules.diagnostic(path.span));
                }
                ast::ItemKind::Type(declaration) => {
                    self.skip_unavailable_type(declaration.name, item.span, Feature::CustomTypes)
                }
                ast::ItemKind::Choice(declaration) => {
                    self.skip_unavailable_type(declaration.name, item.span, Feature::Choices)
                }
                ast::ItemKind::Constant(binding) => {
                    let id = self.constants.len();
                    self.constants.push(ConstantEntry {
                        binding,
                        public,
                        state: ConstantState::Pending,
                        used: false,
                    });
                    if binding.name.text != DISCARD {
                        self.declare_global(binding.name, Global::Constant(id));
                    }
                }
                ast::ItemKind::Function(declaration) => {
                    let id = self.functions.len();
                    self.functions.push(FunctionEntry {
                        declaration,
                        parameters: Vec::new(),
                        return_type: Ok(None),
                    });
                    self.declare_global(declaration.name, Global::Function(id));
                }
            }
        }
        for id in 0..self.functions.len() {
            self.resolve_signature(id);
        }
    }

    /// Reports a declaration of a later phase once, so uses of its name are not reported again.
    fn skip_unavailable_type(&mut self, name: Name<'src>, item: Span, feature: Feature) {
        self.unavailable_types.insert(name.text);
        self.report(feature.diagnostic(Span::new(item.start, name.span.end)));
    }

    fn declare_global(&mut self, name: Name<'src>, global: Global) {
        if let Some(built_in) = BuiltIn::of(name.text) {
            self.report(errors::built_in_name(name.text, built_in, name.span));
            return;
        }
        if let Some(&previous) = self.globals.get(name.text) {
            if let Some(previous_span) = self.global_span(previous) {
                self.report(errors::already_declared(
                    name.text,
                    name.span,
                    previous_span,
                ));
            }
            return;
        }
        self.globals.insert(name.text, global);
    }

    fn resolve_signature(&mut self, id: usize) {
        let Some(entry) = self.functions.get(id) else {
            return;
        };
        let declaration = entry.declaration;

        let parameters = declaration
            .parameters
            .iter()
            .map(|parameter| {
                let mutable = parameter.mutability == Mutability::Variable;
                Parameter {
                    name: parameter.name,
                    ty: self.resolve_type(&parameter.ty),
                    mutable,
                    keyword: mutable
                        .then(|| Span::new(parameter.span.start, parameter.ty.span.start)),
                    span: parameter.span,
                }
            })
            .collect();
        let return_type = match &declaration.return_type {
            Some(ty) => self.resolve_type(ty).map(Some),
            None => Ok(None),
        };

        let is_entry_point = declaration.name.text == ENTRY_POINT;
        if is_entry_point
            && (!declaration.parameters.is_empty() || declaration.return_type.is_some())
        {
            self.report(errors::main_form(declaration.name.span));
        }
        if let Some(entry) = self.functions.get_mut(id) {
            entry.parameters = parameters;
            entry.return_type = return_type;
        }
    }

    /// Resolves a type as written in the source.
    pub(super) fn resolve_type(&mut self, ty: &ast::TypeExpr<'src>) -> CheckResult<Type> {
        let (path, arguments) = match &ty.kind {
            ast::TypeExprKind::Packed(_) => {
                return Err(self.report(Feature::Choices.diagnostic(ty.span)));
            }
            ast::TypeExprKind::Named { path, arguments } => (path, arguments),
        };
        let [name] = path.segments.as_slice() else {
            return Err(self.report(Feature::Modules.diagnostic(path.span)));
        };
        if let Some(feature) = Feature::future_type(name.text) {
            return Err(self.report(feature.diagnostic(ty.span)));
        }
        if self.unavailable_types.contains(name.text) {
            return Err(ErrorReported);
        }
        let resolved = match name.text {
            "boolean" => Some(Type::Boolean),
            "string" => Some(Type::String),
            other => IntegerType::from_name(other).map(Type::Integer),
        };
        match resolved {
            Some(resolved) if arguments.is_empty() => Ok(resolved),
            _ if matches!(name.text, "integer" | "unsigned") => {
                Err(self.report(errors::sizeless_integer(name.text, ty.span)))
            }
            _ if is_sized_integer_name(name.text) => Err(self.report(
                errors::unsupported_integer_size(self.text(ty.span), ty.span),
            )),
            _ => {
                let known = IntegerType::ALL
                    .iter()
                    .map(|integer| integer.name())
                    .chain(["boolean", "string"]);
                let suggestion = similarity::closest_match(name.text, known);
                Err(self.report(errors::unknown_type(
                    self.text(ty.span),
                    ty.span,
                    suggestion,
                )))
            }
        }
    }

    pub(super) fn compute_constants(&mut self) {
        for id in 0..self.constants.len() {
            // A failure is reported where it happens; the result is only needed by the constants that use it.
            let _ = self.constant_value(id, None);
        }
    }

    /// The value of a top-level constant, computing it first if needed. `reference` is where it is used.
    pub(super) fn constant_value(
        &mut self,
        id: ConstantId,
        reference: Option<Span>,
    ) -> CheckResult<Value> {
        let Some(entry) = self.constants.get(id) else {
            return Err(ErrorReported);
        };
        let binding = entry.binding;
        match &entry.state {
            ConstantState::Done(value) => return Ok(value.clone()),
            ConstantState::Failed => return Err(ErrorReported),
            ConstantState::InProgress => {
                let span = reference.unwrap_or(binding.name.span);
                let error = errors::depends_on_itself(binding.name.text, span, binding.name.span);
                return Err(self.report(error));
            }
            ConstantState::Pending => {}
        }

        self.set_constant_state(id, ConstantState::InProgress);
        let result = self.compute_constant(binding);
        let state = match &result {
            Ok(value) => ConstantState::Done(value.clone()),
            Err(ErrorReported) => ConstantState::Failed,
        };
        self.set_constant_state(id, state);
        result
    }

    fn set_constant_state(&mut self, id: ConstantId, state: ConstantState) {
        if let Some(entry) = self.constants.get_mut(id) {
            entry.state = state;
        }
    }

    fn compute_constant(&mut self, binding: &'ast ast::Binding<'src>) -> CheckResult<Value> {
        let declared = binding.ty.as_ref().map(|ty| self.resolve_type(ty));
        // Constants are computed outside every function, so names resolve to top-level declarations only.
        let function_scope = self.scope.take();
        let value = self.check_expr(&binding.value, declared.and_then(Result::ok));
        self.scope = function_scope;
        let value = value?;
        match self.known_value(&value) {
            Some(known) => Ok(known),
            None => Err(self.report(errors::not_computable(runtime_part(&value)))),
        }
    }
}

/// True for a name such as `integer128` or `unsigned7`: an integer type with a size Lymra lacks.
pub(super) fn is_sized_integer_name(name: &str) -> bool {
    ["integer", "unsigned"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|size| !size.is_empty() && size.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

/// The part of an expression that is only known at run time: in a checked constant, its first call.
fn runtime_part(expr: &ir::Expr) -> Span {
    match &expr.kind {
        ir::ExprKind::Negate(operand)
        | ir::ExprKind::Not(operand)
        | ir::ExprKind::Convert { operand, .. } => runtime_part(operand),
        ir::ExprKind::Arithmetic { left, right, .. }
        | ir::ExprKind::Compare { left, right, .. }
        | ir::ExprKind::Logic { left, right, .. } => {
            if matches!(
                left.kind,
                ir::ExprKind::Literal(_) | ir::ExprKind::Constant(_)
            ) {
                runtime_part(right)
            } else {
                runtime_part(left)
            }
        }
        ir::ExprKind::Literal(_)
        | ir::ExprKind::Constant(_)
        | ir::ExprKind::Local(_)
        | ir::ExprKind::Call(_) => expr.span,
    }
}
