/*!
 * Error L0704: a list changed where a newer list may sit above it on its data
 * stack (spec section 7.2).
 *
 * Each list binding has a data stack of its own, so within one call of a
 * function its lists never share one. The copies of a binding at different
 * depths of a recursion do: each sits above the copy of the call before it.
 * A call that holds its own copy of a binding can only reach an older copy
 * through a `variable` parameter, and that older copy lies below its own.
 * Only the newest list on a data stack can grow or be replaced, so the call
 * cannot change the older copy while its own one is alive.
 *
 * The pass works out, for every `variable` list parameter, which bindings'
 * lists can reach it through any chain of calls, and whether its function
 * can change it, itself or through the functions it passes it on to. A
 * change to a parameter where a live binding of the same function can be
 * the one it holds, from an earlier call, is the error.
 */

use std::collections::{BTreeSet, HashMap, HashSet};

use super::errors::{self, Change};
use crate::diagnostics::Diagnostic;
use crate::ir::{
    Argument, Block, Call, Expr, ExprKind, Function, FunctionId, LocalId, PlaceStep, PrintPart,
    Statement,
};
use crate::source::Span;

/// A local of one function: a list binding, or a `variable` parameter.
type Owned = (FunctionId, LocalId);

/// Every error L0704 in the program, in source order.
pub(super) fn check(functions: &[Function<'_>]) -> Vec<Diagnostic> {
    let flow = Flow::of(functions);
    let mut diagnostics: Vec<Diagnostic> = functions
        .iter()
        .enumerate()
        .flat_map(|(id, function)| {
            let mut walk = Walk {
                functions,
                function: id,
                flow: &flow,
                live: Vec::new(),
                diagnostics: Vec::new(),
            };
            walk.block(&function.body);
            walk.diagnostics
        })
        .collect();
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.span.start);
    diagnostics
}

/// Whether `local` of `function` holds a list.
fn is_list(function: &Function<'_>, local: LocalId) -> bool {
    function.locals[local].ty.is_growable()
}

/// Which lists reach which `variable` list parameters, and which of those parameters can change.
#[derive(Default)]
struct Flow {
    /// The list bindings whose lists can reach each parameter.
    reaching: HashMap<Owned, BTreeSet<Owned>>,
    /// The parameters whose function can change the list in them, itself or through a call.
    changing: HashSet<Owned>,
    /// For each parameter, the parameters of called functions it is passed on to.
    passed_to: HashMap<Owned, Vec<Owned>>,
    /// For each parameter, the parameters of callers that pass theirs on to it.
    passed_from: HashMap<Owned, Vec<Owned>>,
}

impl Flow {
    fn of(functions: &[Function<'_>]) -> Self {
        let mut flow = Self::default();
        for (id, function) in functions.iter().enumerate() {
            let mut edges = Edges {
                functions,
                caller: id,
                flow: &mut flow,
            };
            edges.block(&function.body);
        }
        flow.spread_reaching();
        flow.spread_changing();
        flow
    }

    /// Carries each parameter's bindings on to the parameters it is passed to, until nothing more arrives.
    fn spread_reaching(&mut self) {
        let mut waiting: Vec<Owned> = self.reaching.keys().copied().collect();
        while let Some(parameter) = waiting.pop() {
            let Some(bindings) = self.reaching.get(&parameter).cloned() else {
                continue;
            };
            for &next in self.passed_to.get(&parameter).into_iter().flatten() {
                let known = self.reaching.entry(next).or_default();
                let before = known.len();
                known.extend(bindings.iter().copied());
                if known.len() > before {
                    waiting.push(next);
                }
            }
        }
    }

    /// Marks every parameter passed on to one that can change as one that can change too.
    fn spread_changing(&mut self) {
        let mut waiting: Vec<Owned> = self.changing.iter().copied().collect();
        while let Some(parameter) = waiting.pop() {
            for &earlier in self.passed_from.get(&parameter).into_iter().flatten() {
                if self.changing.insert(earlier) {
                    waiting.push(earlier);
                }
            }
        }
    }
}

/// Collects where lists are passed with `variable` and where parameters are changed.
struct Edges<'a, 'src> {
    functions: &'a [Function<'src>],
    caller: FunctionId,
    flow: &'a mut Flow,
}

impl Edges<'_, '_> {
    fn block(&mut self, block: &Block) {
        for statement in &block.statements {
            let mut calls = Vec::new();
            statement_calls(statement, &mut calls);
            for call in calls {
                self.call(call);
            }
            match statement {
                Statement::Push { list, .. } => self.change(*list),
                Statement::Assign { target, value }
                    if target.steps.is_empty() && value.ty.is_growable() =>
                {
                    self.change(target.local);
                }
                _ => {}
            }
            for body in statement_blocks(statement) {
                self.block(body);
            }
        }
    }

    fn change(&mut self, local: LocalId) {
        if self.functions[self.caller].locals[local].by_reference {
            self.flow.changing.insert((self.caller, local));
        }
    }

    fn call(&mut self, call: &Call) {
        let caller = &self.functions[self.caller];
        let parameters = &self.functions[call.function].parameters;
        for (argument, &parameter) in call.arguments.iter().zip(parameters) {
            let Argument::Reference { local, .. } = *argument else {
                continue;
            };
            if !is_list(caller, local) {
                continue;
            }
            let from = (self.caller, local);
            let to = (call.function, parameter);
            if caller.locals[local].by_reference {
                self.flow.passed_to.entry(from).or_default().push(to);
                self.flow.passed_from.entry(to).or_default().push(from);
            } else {
                self.flow.reaching.entry(to).or_default().insert(from);
            }
        }
    }
}

/// Walks one function in order, keeping the list bindings alive at each point.
struct Walk<'a, 'src> {
    functions: &'a [Function<'src>],
    function: FunctionId,
    flow: &'a Flow,
    /// The list bindings declared so far in the blocks around the current point.
    live: Vec<LocalId>,
    diagnostics: Vec<Diagnostic>,
}

impl Walk<'_, '_> {
    fn block(&mut self, block: &Block) {
        let declared_before = self.live.len();
        for statement in &block.statements {
            self.statement(statement);
        }
        self.live.truncate(declared_before);
    }

    fn statement(&mut self, statement: &Statement) {
        match statement {
            Statement::Declare { local, value } => {
                // The binding gets its list once the value is built, so it is not above anything before.
                self.expr(value);
                if is_list(&self.functions[self.function], *local) {
                    self.live.push(*local);
                }
            }
            Statement::Assign { target, value } => {
                for step in &target.steps {
                    if let PlaceStep::Index(index) = step {
                        self.expr(&index.value);
                    }
                }
                if !(target.steps.is_empty() && value.ty.is_growable()) {
                    self.expr(value);
                    return;
                }
                // A new list that does not read the old one is built after the old one is freed (spec 7.9).
                if value.mentions(target.local) {
                    self.expr(value);
                } else {
                    let live = self.live.clone();
                    self.live.retain(|&binding| binding != target.local);
                    self.expr(value);
                    self.live = live;
                }
                self.change(target.local, Change::Replace, target.span);
            }
            Statement::Push { list, value, span } => {
                self.expr(value);
                self.change(*list, Change::Push, *span);
            }
            _ => {
                let mut calls = Vec::new();
                statement_calls(statement, &mut calls);
                for call in calls {
                    self.call(call);
                }
                for body in statement_blocks(statement) {
                    self.block(body);
                }
            }
        }
    }

    fn expr(&mut self, expr: &Expr) {
        let mut calls = Vec::new();
        expr_calls(expr, &mut calls);
        for call in calls {
            self.call(call);
        }
    }

    fn call(&mut self, call: &Call) {
        let callee = &self.functions[call.function];
        for (argument, &parameter) in call.arguments.iter().zip(&callee.parameters) {
            if let Argument::Reference { local, span } = *argument {
                if self.flow.changing.contains(&(call.function, parameter)) {
                    self.change(local, Change::Pass(callee.name), span);
                }
            }
        }
    }

    /// Reports a change to `local` at `span` if a live binding of this function can be the list it holds.
    fn change(&mut self, local: LocalId, change: Change<'_>, span: Span) {
        let Some(reaching) = self.flow.reaching.get(&(self.function, local)) else {
            return;
        };
        let function = &self.functions[self.function];
        let newer = self
            .live
            .iter()
            .find(|&&binding| reaching.contains(&(self.function, binding)));
        if let Some(&binding) = newer {
            let binding = &function.locals[binding];
            self.diagnostics.push(errors::changed_below_newer(
                function.locals[local].name,
                change,
                span,
                (binding.name, binding.span),
            ));
        }
    }
}

/// Every call a statement makes outside the blocks it holds.
fn statement_calls<'a>(statement: &'a Statement, calls: &mut Vec<&'a Call>) {
    match statement {
        Statement::Declare { value, .. }
        | Statement::Discard(value)
        | Statement::Push { value, .. }
        | Statement::Return(Some(value))
        | Statement::Assert { condition: value }
        | Statement::While {
            condition: value, ..
        }
        | Statement::ForEach { array: value, .. }
        | Statement::Match { value, .. } => expr_calls(value, calls),
        Statement::Assign { target, value } => {
            for step in &target.steps {
                if let PlaceStep::Index(index) = step {
                    expr_calls(&index.value, calls);
                }
            }
            expr_calls(value, calls);
        }
        Statement::Call(call) => call_calls(call, calls),
        Statement::Print(parts) => {
            for part in parts {
                if let PrintPart::Value { value, .. } = part {
                    expr_calls(value, calls);
                }
            }
        }
        Statement::If { branches, .. } => {
            for branch in branches {
                expr_calls(&branch.condition, calls);
            }
        }
        Statement::ForRange { start, end, .. } => {
            expr_calls(start, calls);
            expr_calls(end, calls);
        }
        Statement::Return(None) | Statement::Break | Statement::Continue => {}
    }
}

/// The blocks a statement holds, in order.
fn statement_blocks(statement: &Statement) -> Vec<&Block> {
    match statement {
        Statement::If {
            branches,
            otherwise,
        } => branches
            .iter()
            .map(|branch| &branch.body)
            .chain(otherwise)
            .collect(),
        Statement::While { body, .. }
        | Statement::ForRange { body, .. }
        | Statement::ForEach { body, .. } => vec![body],
        Statement::Match { cases, .. } => cases.iter().map(|case| &case.body).collect(),
        _ => Vec::new(),
    }
}

/// `call` and every call inside its arguments.
fn call_calls<'a>(call: &'a Call, calls: &mut Vec<&'a Call>) {
    calls.push(call);
    for argument in &call.arguments {
        if let Argument::Value(value) = argument {
            expr_calls(value, calls);
        }
    }
}

/// Every call inside an expression.
fn expr_calls<'a>(expr: &'a Expr, calls: &mut Vec<&'a Call>) {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Local(_) | ExprKind::Constant(_) | ExprKind::EmptyList => {
        }
        ExprKind::Call(call) => call_calls(call, calls),
        ExprKind::Negate { operand, .. }
        | ExprKind::Not(operand)
        | ExprKind::Convert { operand, .. }
        | ExprKind::Fill(operand)
        | ExprKind::Length(operand)
        | ExprKind::Field {
            object: operand, ..
        } => expr_calls(operand, calls),
        ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => {
            for field in fields {
                expr_calls(&field.value, calls);
            }
        }
        ExprKind::Arithmetic { left, right, .. }
        | ExprKind::Compare { left, right, .. }
        | ExprKind::Logic { left, right, .. }
        | ExprKind::Wrapping { left, right, .. }
        | ExprKind::Index {
            array: left,
            index: right,
            ..
        } => {
            expr_calls(left, calls);
            expr_calls(right, calls);
        }
    }
}
