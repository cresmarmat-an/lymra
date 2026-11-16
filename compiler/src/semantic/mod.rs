/*!
 * The semantic checker: resolves names, checks types and control flow, and
 * builds the checked program that code generation reads.
 *
 * It runs on a file whose syntax is valid. Like the parser, it never stops at
 * the first error. A failed expression is reported once and travels up as
 * `ErrorReported`, the statement around it is dropped, and checking goes on
 * with the next statement. The checked program is only returned when no
 * error was found.
 */

mod arrays;
mod choices;
mod custom_types;
mod cycles;
mod data_stacks;
mod errors;
mod evaluation_order;
mod expressions;
mod failures;
mod features;
mod floats;
mod handles;
mod items;
mod lists;
mod places;
mod pools;
mod roots;
mod scope;
mod statements;
mod system;
mod values;
mod views;
mod walk;
mod wrapping;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;

use crate::ast::{self, Name, OptimizeMode};
use crate::diagnostics::{Diagnostic, ErrorReported, Help};
use crate::ir::{self, ChoiceType, ConstantId, FunctionId, IntegerType, Program, Type, Value};
use crate::similarity;
use crate::source::{SourceFile, Span};
use scope::{FunctionScope, LocalKind};
use system::BuiltInChoice;

type CheckResult<T> = Result<T, ErrorReported>;

/// The built-in functions the compiler implements.
const BUILT_IN_FUNCTIONS: [&str; 17] = [
    "print",
    "read_file",
    "write_file",
    "read_input",
    "read_line",
    "range",
    "length",
    "push",
    "pop",
    "top",
    "insert",
    "remove",
    "slice",
    "bytes",
    "wrapping_add",
    "wrapping_subtract",
    "wrapping_multiply",
];

/// Built-in types that are not integers. Calling `array`, `list`, `stack` or `pool` builds one.
const OTHER_TYPE_NAMES: [&str; 10] = [
    "boolean", "string", "array", "list", "stack", "view", "pool", "handle", "float32", "float64",
];

/// What a name the language itself defines stands for. No declaration can take such a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuiltIn {
    Function,
    /// A type. Calling an integer type, as in `integer32(count)`, converts a value to it.
    Type,
    /// A variant of a choice the language declares, such as `NoSuchFile`.
    Variant,
}

impl BuiltIn {
    fn of(name: &str) -> Option<BuiltIn> {
        let choice = BuiltInChoice::ALL
            .into_iter()
            .find(|choice| choice.names(name));
        if BUILT_IN_FUNCTIONS.contains(&name) {
            Some(BuiltIn::Function)
        } else if IntegerType::from_name(name).is_some()
            || OTHER_TYPE_NAMES.contains(&name)
            || choice.is_some_and(|choice| choice.name() == name)
        {
            Some(BuiltIn::Type)
        } else if choice.is_some() {
            Some(BuiltIn::Variant)
        } else {
            None
        }
    }

    fn noun(self) -> &'static str {
        match self {
            BuiltIn::Function => "function",
            BuiltIn::Type => "type",
            BuiltIn::Variant => "variant",
        }
    }
}

/// The name that declares a binding nobody reads.
const DISCARD: &str = "_";

/// Checks a parsed file and returns the checked program, or every error found.
pub fn check<'src>(
    source: &'src SourceFile,
    module: &ast::Module<'src>,
) -> Result<Program<'src>, Vec<Diagnostic>> {
    let mut checker = Checker::new(source);
    checker.read_directives(&module.directives);
    checker.collect_items(&module.items);
    checker.compute_constants();
    let functions = checker.check_functions();
    checker.report_unused_constants();
    checker.finish(functions)
}

/// The error for a file without `main`, which only matters when the file is built or run.
pub fn missing_main_error() -> Diagnostic {
    errors::missing_main(Span::empty(0))
}

/// Collects every name an expression mentions, including inside the blocks it contains.
#[derive(Default)]
struct NameCollector<'src> {
    names: Vec<&'src str>,
}

impl<'src> ast::visit::Visitor<'src> for NameCollector<'src> {
    fn visit_expr(&mut self, expr: &ast::Expr<'src>) {
        if let ast::ExprKind::Name(name) = &expr.kind {
            self.names.push(name);
        }
        ast::visit::walk_expr(self, expr);
    }
}

/// Index into `Checker::types`.
type TypeId = usize;

/// A declaration the checker works out when something first needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Definition {
    Type(TypeId),
    Constant(ConstantId),
}

/// A name declared at the top level of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Global {
    Function(FunctionId),
    Constant(ConstantId),
    Type(TypeId),
    /// A variant of the `choice` declared as the type `choice`, by its position there.
    Variant {
        choice: TypeId,
        variant: usize,
    },
}

/// A parameter with its type resolved.
#[derive(Clone, Debug)]
struct Parameter<'src> {
    name: Name<'src>,
    ty: CheckResult<Type>,
    mutable: bool,
    /// The `variable ` before the type, including the space, for fixes that remove it.
    keyword: Option<Span>,
    span: Span,
}

struct FunctionEntry<'src, 'ast> {
    declaration: &'ast ast::Function<'src>,
    parameters: Vec<Parameter<'src>>,
    return_type: CheckResult<Option<Type>>,
    /// What the function can fail with, after `fails`, or `Ok(None)` for a function that cannot fail.
    fails: CheckResult<Option<Type>>,
}

impl FunctionEntry<'_, '_> {
    /// What a call gives: what the function returns, or for one that can fail, its result (spec 8).
    fn call_type(&self) -> CheckResult<Option<Type>> {
        let returns = self.return_type.clone()?;
        Ok(match self.fails.clone()? {
            Some(error) => Some(Type::Choice(Arc::new(ChoiceType::result(returns, error)))),
            None => returns,
        })
    }
}

#[derive(Debug)]
enum ConstantState {
    Pending,
    InProgress,
    Done(Value),
    Failed,
}

/// How far a type declaration is resolved. Resolving one in progress means it contains itself.
#[derive(Debug)]
enum TypeState {
    Pending,
    InProgress,
    Done(Type),
    Failed,
}

/// A type the program declares: a `type` with fields, or a `choice` with variants.
#[derive(Clone, Copy, Debug)]
enum DeclaredType<'src, 'ast> {
    Record(&'ast ast::TypeDeclaration<'src>),
    Choice(&'ast ast::ChoiceDeclaration<'src>),
    /// A choice the language declares itself, written nowhere in the program.
    BuiltIn(BuiltInChoice),
}

impl<'src> DeclaredType<'src, '_> {
    fn name(self) -> Name<'src> {
        match self {
            Self::Record(declaration) => declaration.name,
            Self::Choice(declaration) => declaration.name,
            Self::BuiltIn(choice) => Name {
                text: choice.name(),
                span: Span::empty(0),
            },
        }
    }
}

struct TypeEntry<'src, 'ast> {
    declaration: DeclaredType<'src, 'ast>,
    state: TypeState,
}

struct ConstantEntry<'src, 'ast> {
    binding: &'ast ast::Binding<'src>,
    public: bool,
    state: ConstantState,
    used: bool,
}

struct Checker<'src, 'ast> {
    source: &'src SourceFile,
    diagnostics: Vec<Diagnostic>,
    globals: HashMap<&'src str, Global>,
    functions: Vec<FunctionEntry<'src, 'ast>>,
    constants: Vec<ConstantEntry<'src, 'ast>>,
    types: Vec<TypeEntry<'src, 'ast>>,
    /// The declarations being worked out, innermost last, each with where the one before it names it.
    resolving: Vec<(Definition, Span)>,
    /**
     * Set while an expression is checked only for its own mistakes. Where
     * its value goes is unknown then, so a mistake that depends on that,
     * such as an `array(value)` with no length to take, is not reported.
     */
    destination_unknown: bool,
    optimize: Option<OptimizeMode>,
    prove_bounds: bool,
    /// The function whose body is being checked. `None` while top-level constants are computed.
    scope: Option<FunctionScope<'src>>,
    /// The value of the statement being checked, where a `try` may have `else fail` (spec 8).
    whole_value: Option<Span>,
}

impl<'src, 'ast> Checker<'src, 'ast> {
    fn new(source: &'src SourceFile) -> Self {
        Self {
            source,
            diagnostics: Vec::new(),
            globals: HashMap::new(),
            functions: Vec::new(),
            constants: Vec::new(),
            types: Vec::new(),
            resolving: Vec::new(),
            destination_unknown: false,
            optimize: None,
            prove_bounds: false,
            scope: None,
            whole_value: None,
        }
    }

    fn report(&mut self, diagnostic: Diagnostic) -> ErrorReported {
        self.diagnostics.push(diagnostic);
        ErrorReported
    }

    fn text(&self, span: Span) -> &'src str {
        self.source.slice(span)
    }

    fn read_directives(&mut self, directives: &[ast::Directive]) {
        for directive in directives {
            match directive.kind {
                ast::DirectiveKind::Optimize(mode) => self.optimize = Some(mode),
                ast::DirectiveKind::ProveBounds => self.prove_bounds = true,
                ast::DirectiveKind::Edition(_) => {}
            }
        }
    }

    /// The value of the expression if it is known at compile time.
    fn known_value(&self, expr: &ir::Expr) -> Option<Value> {
        match &expr.kind {
            ir::ExprKind::Literal(value) => Some(value.clone()),
            ir::ExprKind::Constant(id) => match self.constants.get(*id).map(|entry| &entry.state) {
                Some(ConstantState::Done(value)) => Some(value.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Where a top-level name was declared.
    fn global_span(&self, global: Global) -> Option<Span> {
        match global {
            Global::Function(id) => self
                .functions
                .get(id)
                .map(|entry| entry.declaration.name.span),
            Global::Constant(id) => self.constants.get(id).map(|entry| entry.binding.name.span),
            Global::Type(id) => match self.types.get(id)?.declaration {
                DeclaredType::BuiltIn(_) => None,
                declaration @ (DeclaredType::Record(_) | DeclaredType::Choice(_)) => {
                    Some(declaration.name().span)
                }
            },
            Global::Variant { choice, variant } => match self.types.get(choice)?.declaration {
                DeclaredType::Choice(declaration) => declaration
                    .variants
                    .get(variant)
                    .map(|variant| variant.name.span),
                DeclaredType::Record(_) | DeclaredType::BuiltIn(_) => None,
            },
        }
    }

    /// The declaration that already uses `name` where a new one would be visible.
    fn visible_declaration(&self, name: &str) -> Option<Span> {
        let local = self.scope.as_ref().and_then(|scope| {
            let id = scope.lookup(name)?;
            scope.local(id).map(|local| local.name.span)
        });
        local.or_else(|| {
            let global = self.globals.get(name).copied()?;
            self.global_span(global)
        })
    }

    /// Declares a parameter, binding or loop variable in the function being checked.
    fn declare_local(
        &mut self,
        name: Name<'src>,
        ty: CheckResult<Type>,
        kind: LocalKind,
        keyword: Option<Span>,
        declaration_failed: bool,
    ) -> CheckResult<ir::LocalId> {
        if name.text != DISCARD {
            if let Some(built_in) = BuiltIn::of(name.text) {
                let error = self.report(errors::built_in_name(name.text, built_in, name.span));
                /*
                 * The binding still goes in scope, without a type, so the code
                 * that uses it is checked without the same mistake reported
                 * again at every use.
                 */
                let rejected = scope::Local {
                    name,
                    ty: Err(error),
                    kind,
                    keyword,
                    read: false,
                    written: false,
                    declaration_failed: true,
                };
                if let Some(scope) = self.scope.as_mut() {
                    scope.declare(rejected);
                }
                return Err(error);
            }
            if let Some(previous) = self.visible_declaration(name.text) {
                return Err(self.report(errors::already_declared(name.text, name.span, previous)));
            }
        }
        let local = scope::Local {
            name,
            ty,
            kind,
            keyword,
            read: false,
            written: false,
            declaration_failed,
        };
        match self.scope.as_mut() {
            Some(scope) => Ok(scope.declare(local)),
            None => Err(ErrorReported),
        }
    }

    /**
     * Reports a name that resolves to nothing, with the closest visible name
     * as a suggestion. The suggested binding counts as read, so the typo does
     * not also make it look unused.
     */
    fn report_unknown_name(&mut self, name: &str, span: Span) -> ErrorReported {
        self.report_unknown(name, span, &[])
    }

    /// Reports a called name that resolves to nothing. Calling an integer type converts to it, so those are suggested too.
    fn report_unknown_callee(&mut self, name: &str, span: Span) -> ErrorReported {
        self.report_unknown(name, span, &IntegerType::ALL.map(IntegerType::name))
    }

    /// Reports an unknown name, suggesting the closest of the visible names, the built-in functions and `more`.
    fn report_unknown(&mut self, name: &str, span: Span, more: &[&'src str]) -> ErrorReported {
        let locals = self.scope.iter().flat_map(|scope| scope.visible_names());
        let candidates = locals
            .chain(self.globals.keys().copied())
            .chain(BUILT_IN_FUNCTIONS)
            .chain(more.iter().copied());
        let suggestion = similarity::closest_match(name, candidates);
        if let Some(intended) = suggestion {
            self.mark_name_read(intended);
        }
        self.report(errors::unknown_name(name, span, suggestion))
    }

    /// Records that `name` is read, if it is a local or a top-level constant.
    fn mark_name_read(&mut self, name: &str) {
        let local = self.scope.as_ref().and_then(|scope| scope.lookup(name));
        if let Some(local) = local.and_then(|id| self.scope.as_mut()?.local_mut(id)) {
            local.read = true;
            return;
        }
        if let Some(Global::Constant(id)) = self.globals.get(name).copied() {
            if let Some(entry) = self.constants.get_mut(id) {
                entry.used = true;
            }
        }
    }

    /// Records that a statement meant to change the local `name`, if it is a `variable`.
    fn mark_name_changed(&mut self, name: &str) {
        let local = self.scope.as_ref().and_then(|scope| scope.lookup(name));
        if let Some(local) = local.and_then(|id| self.scope.as_mut()?.local_mut(id)) {
            if local.kind.is_mutable() {
                local.written = true;
            }
        }
    }

    /**
     * Records every binding an expression reads, without checking it. Used
     * inside a construct that is rejected as a whole, so the rejection is the
     * only error, and the bindings it uses do not look unused.
     */
    fn mark_reads(&mut self, expr: &ast::Expr<'src>) {
        let mut collector = NameCollector::default();
        ast::visit::Visitor::visit_expr(&mut collector, expr);
        for name in collector.names {
            self.mark_name_read(name);
        }
    }

    fn check_functions(&mut self) -> Vec<CheckResult<ir::Function<'src>>> {
        (0..self.functions.len())
            .map(|id| self.check_function(id))
            .collect()
    }

    fn check_function(&mut self, id: FunctionId) -> CheckResult<ir::Function<'src>> {
        let Some(entry) = self.functions.get(id) else {
            return Err(ErrorReported);
        };
        let declaration = entry.declaration;
        let parameters = entry.parameters.clone();
        let return_type = entry.return_type.clone();
        let fails = entry.fails.clone();
        let call_type = entry.call_type();
        let result = match (&fails, &call_type) {
            (Ok(Some(_)), Ok(Some(result))) => Some(result.clone()),
            _ => None,
        };
        self.scope = Some(FunctionScope::new(
            declaration.name,
            return_type.clone(),
            fails,
            result.clone(),
        ));

        let mut parameter_ids = Vec::new();
        let mut parameters_failed = false;
        for parameter in parameters {
            let kind = if parameter.mutable {
                LocalKind::VariableParameter
            } else {
                LocalKind::Parameter
            };
            match self.declare_local(parameter.name, parameter.ty, kind, parameter.keyword, false) {
                Ok(local) => parameter_ids.push(local),
                Err(ErrorReported) => parameters_failed = true,
            }
        }

        let (mut body, falls_through) = self.check_block(&declaration.body);
        if let Ok(Some(return_type)) = &return_type {
            if falls_through {
                let error = errors::missing_return(
                    declaration.name.text,
                    return_type,
                    declaration.name.span,
                );
                self.report(error);
            }
        }
        // A function that can fail and returns nothing succeeds when it reaches its end.
        if let (Ok(None), Some(result), true) = (&return_type, &result, falls_through) {
            let end = Span::empty(declaration.body.span.end);
            let success = failures::success(None, result, end);
            body.statements.push(ir::Statement::Return(Some(success)));
        }

        let closed = self
            .scope
            .as_mut()
            .map(FunctionScope::close_block)
            .unwrap_or_default();
        self.report_scope_exit(&closed);
        let scope = self.scope.take().ok_or(ErrorReported)?;
        if parameters_failed {
            return Err(ErrorReported);
        }
        Ok(ir::Function {
            name: declaration.name.text,
            parameters: parameter_ids,
            return_type: call_type?,
            locals: scope.into_locals()?,
            body,
        })
    }

    fn report_unused_constants(&mut self) {
        let unused: Vec<Name<'src>> = self
            .constants
            .iter()
            .filter(|entry| {
                !entry.public && !entry.used && matches!(entry.state, ConstantState::Done(_))
            })
            .map(|entry| entry.binding.name)
            .filter(|name| name.text != DISCARD)
            .collect();
        for name in unused {
            let help = Help::text("use it, or remove its declaration");
            self.report(errors::unused(name.text, "constant", name.span, help));
        }
    }

    fn finish(
        mut self,
        functions: Vec<CheckResult<ir::Function<'src>>>,
    ) -> Result<Program<'src>, Vec<Diagnostic>> {
        if !self.diagnostics.is_empty() {
            // A stable sort keeps errors at the same place in the order they were found.
            self.diagnostics
                .sort_by_key(|diagnostic| diagnostic.primary.span.start);
            return Err(self.diagnostics);
        }
        let functions: CheckResult<Vec<ir::Function<'src>>> = functions.into_iter().collect();
        let constants: CheckResult<Vec<ir::Constant<'src>>> = self
            .constants
            .into_iter()
            .map(|entry| match entry.state {
                ConstantState::Done(value) => Ok(ir::Constant {
                    name: entry.binding.name.text,
                    value,
                }),
                ConstantState::Pending | ConstantState::InProgress | ConstantState::Failed => {
                    Err(ErrorReported)
                }
            })
            .collect();
        let main = match self.globals.get("main") {
            Some(Global::Function(id)) => Some(*id),
            _ => None,
        };
        match (functions, constants) {
            (Ok(functions), Ok(constants)) => {
                // Where lists, views and handles can travel is known only once every function is checked.
                let mut unsafe_memory = data_stacks::check(&functions);
                let (handle_errors, pools) = handles::check(&functions);
                unsafe_memory.extend(handle_errors);
                unsafe_memory.extend(roots::check(&functions, &pools));
                if !unsafe_memory.is_empty() {
                    unsafe_memory.sort_by_key(|diagnostic| diagnostic.primary.span.start);
                    return Err(unsafe_memory);
                }
                Ok(Program {
                    constants,
                    functions,
                    main,
                    optimize: self.optimize,
                    prove_bounds: self.prove_bounds,
                })
            }
            // Every failure is reported where it happens, so this cannot occur without an error above.
            _ => Err(self.diagnostics),
        }
    }
}
