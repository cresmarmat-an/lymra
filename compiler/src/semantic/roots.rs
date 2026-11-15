/*!
 * What each view shows, and the errors that keep a view from showing memory
 * that is freed or replaced (spec 7.4).
 *
 * A view's root is what keeps the elements it shows: a binding of the
 * function, a parameter, whose root the caller keeps alive through the
 * call, text in the program itself, or a value a statement computes and
 * frees when it ends. The pass works out every root each view of a function
 * can have, from everything it is given anywhere. A function that returns a
 * view returns one rooted in some of its parameters, so a call's result has
 * the roots of what it passed there; across the program, this is worked out
 * to a fixed point.
 *
 * - L0701: a view outlives its root. A binding holds a view of a value its
 *   statement frees, or of a binding whose block ends first; a function
 *   returns a view of its own binding; or a `variable` view parameter is
 *   given one, which the caller would keep.
 * - L0702: a stack is popped, or passed with `variable` to a function that
 *   can pop it, while a view of it is still used, or while a `for` loop goes
 *   over it: the view or the loop could read the element taken off.
 * - L0703: a list is replaced, or passed with `variable` to a function that
 *   can replace it, while a view of it is still used, or while a `for` loop
 *   goes over it. A `push` keeps the elements where they are, so it is fine.
 */

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::errors::{self, Freeing};
use super::walk::{statement_blocks, statement_calls, statement_pops};
use crate::diagnostics::Diagnostic;
use crate::ir::{
    Argument, Block, Call, Expr, ExprKind, Function, LocalId, PlaceStep, PrintPart, Sequence,
    Statement, Type,
};
use crate::source::Span;

/// What keeps the elements a view shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Root {
    /// A local of the function: a binding, or a parameter.
    Local(LocalId),
    /// Text in the program itself, which lasts as long as the program.
    Static,
    /// A value a statement computes without a binding, freed when the statement ends.
    Temporary,
}

type Roots = BTreeSet<Root>;

/// What a function does with views and lists, as its callers see it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Summary {
    /// The positions of the parameters whose roots a view it returns can have.
    returns: BTreeSet<usize>,
    /// For each `variable` view parameter, by position, the positions of the parameters whose roots it can take.
    takes: BTreeMap<usize, BTreeSet<usize>>,
    /// The positions of the `variable` list parameters it can replace, itself or through a call.
    replaces: BTreeSet<usize>,
    /// The positions of the `variable` stack parameters it can pop, itself or through a call.
    pops: BTreeSet<usize>,
}

/// Every error L0701, L0702 and L0703 in the program, in source order.
pub(super) fn check(functions: &[Function<'_>]) -> Vec<Diagnostic> {
    let mut summaries = vec![Summary::default(); functions.len()];
    loop {
        let next: Vec<Summary> = functions
            .iter()
            .map(|function| Analysis::of(function, functions, &summaries).summary())
            .collect();
        if next == summaries {
            break;
        }
        summaries = next;
    }
    let mut diagnostics: Vec<Diagnostic> = functions
        .iter()
        .flat_map(|function| Analysis::of(function, functions, &summaries).diagnostics())
        .collect();
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.span.start);
    diagnostics
}

fn is_view(function: &Function<'_>, local: LocalId) -> bool {
    function.locals[local].ty.as_view().is_some()
}

fn is_list(function: &Function<'_>, local: LocalId) -> bool {
    function.locals[local].ty.as_list().is_some()
}

/// The roots of every view of one function.
struct Analysis<'a, 'src> {
    function: &'a Function<'src>,
    /// Every function of the program, which calls name.
    functions: &'a [Function<'src>],
    summaries: &'a [Summary],
    roots: HashMap<LocalId, Roots>,
}

impl<'a, 'src> Analysis<'a, 'src> {
    /// Gives each view of `function` the roots of everything it is given, until nothing more arrives.
    fn of(
        function: &'a Function<'src>,
        functions: &'a [Function<'src>],
        summaries: &'a [Summary],
    ) -> Self {
        let mut analysis = Self {
            function,
            functions,
            summaries,
            roots: HashMap::new(),
        };
        for &parameter in &function.parameters {
            if is_view(function, parameter) {
                analysis
                    .roots
                    .insert(parameter, Roots::from([Root::Local(parameter)]));
            }
        }
        loop {
            let before = analysis.roots.clone();
            analysis.gather(&function.body);
            if analysis.roots == before {
                break;
            }
        }
        analysis
    }

    fn gather(&mut self, block: &Block) {
        for statement in &block.statements {
            match statement {
                Statement::Declare { local, value } => self.give(*local, value),
                Statement::Assign { target, value } if target.steps.is_empty() => {
                    self.give(target.local, value);
                }
                _ => {}
            }
            let mut calls = Vec::new();
            statement_calls(statement, &mut calls);
            for call in calls {
                for (view, roots, _) in self.taken(call) {
                    self.roots.entry(view).or_default().extend(roots);
                }
            }
            for body in statement_blocks(statement) {
                self.gather(body);
            }
        }
    }

    /// Adds the roots of `value` to those of `local`, if it is a view.
    fn give(&mut self, local: LocalId, value: &Expr) {
        if is_view(self.function, local) {
            let roots = self.roots_of(value);
            self.roots.entry(local).or_default().extend(roots);
        }
    }

    /**
     * The views a call can give new roots, through its `variable` view
     * parameters: each view, with those roots and where it is passed.
     */
    fn taken(&self, call: &Call) -> Vec<(LocalId, Roots, Span)> {
        self.summaries[call.function]
            .takes
            .iter()
            .filter_map(|(&position, sources)| {
                let Some(&Argument::Reference { local, span }) = call.arguments.get(position)
                else {
                    return None;
                };
                let roots = sources
                    .iter()
                    .flat_map(|&source| self.argument_roots(call, source))
                    .collect();
                Some((local, roots, span))
            })
            .collect()
    }

    fn argument_roots(&self, call: &Call, position: usize) -> Roots {
        match call.arguments.get(position) {
            Some(Argument::Value(value)) => self.roots_of(value),
            Some(Argument::Reference { local, .. }) => self.local_roots(*local),
            None => Roots::new(),
        }
    }

    /// The roots a view of `local` has: its own, for a view, or else `local` itself.
    fn local_roots(&self, local: LocalId) -> Roots {
        if is_view(self.function, local) {
            self.roots.get(&local).cloned().unwrap_or_default()
        } else {
            Roots::from([Root::Local(local)])
        }
    }

    /// The roots of a view made from `expr`: what keeps the elements it would show.
    fn roots_of(&self, expr: &Expr) -> Roots {
        // Text values all lie in the program itself so far.
        if expr.ty == Type::String {
            return Roots::from([Root::Static]);
        }
        match &expr.kind {
            ExprKind::Local(local) => self.local_roots(*local),
            ExprKind::Field { object: inner, .. }
            | ExprKind::Index { array: inner, .. }
            | ExprKind::View(inner)
            | ExprKind::Slice { items: inner, .. } => self.roots_of(inner),
            ExprKind::Bytes(_) | ExprKind::Literal(_) | ExprKind::Constant(_) => {
                Roots::from([Root::Static])
            }
            ExprKind::Call(call) if expr.ty.as_view().is_some() => self.summaries[call.function]
                .returns
                .iter()
                .flat_map(|&position| self.argument_roots(call, position))
                .collect(),
            // Anything else computes a new value, which its statement frees.
            _ => Roots::from([Root::Temporary]),
        }
    }

    /// The position of `local` among the function's parameters, if it is one.
    fn parameter_position(&self, local: LocalId) -> Option<usize> {
        self.function
            .parameters
            .iter()
            .position(|&parameter| parameter == local)
    }

    fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        self.summarize(&self.function.body, &mut summary);
        for (position, &parameter) in self.function.parameters.iter().enumerate() {
            let local = &self.function.locals[parameter];
            if local.by_reference && local.ty.as_view().is_some() {
                let sources = self.positions_of(&self.local_roots(parameter));
                summary.takes.insert(position, sources);
            }
        }
        summary
    }

    /// The positions of the parameters among `roots`.
    fn positions_of(&self, roots: &Roots) -> BTreeSet<usize> {
        roots
            .iter()
            .filter_map(|root| match root {
                Root::Local(local) => self.parameter_position(*local),
                Root::Static | Root::Temporary => None,
            })
            .collect()
    }

    /// Adds what the statements of `block` return, replace and pop to `summary`.
    fn summarize(&self, block: &Block, summary: &mut Summary) {
        for statement in &block.statements {
            match statement {
                Statement::Return(Some(value)) if value.ty.as_view().is_some() => {
                    let returned = self.positions_of(&self.roots_of(value));
                    summary.returns.extend(returned);
                }
                Statement::Assign { target, .. }
                    if target.steps.is_empty() && is_list(self.function, target.local) =>
                {
                    summary
                        .replaces
                        .extend(self.parameter_position(target.local));
                }
                _ => {}
            }
            for (stack, _) in statement_pops(statement) {
                summary.pops.extend(self.parameter_position(stack));
            }
            let mut calls = Vec::new();
            statement_calls(statement, &mut calls);
            for call in calls {
                let callee = &self.summaries[call.function];
                for (list, _) in passed_at(call, &callee.replaces) {
                    summary.replaces.extend(self.parameter_position(list));
                }
                for (stack, _) in passed_at(call, &callee.pops) {
                    summary.pops.extend(self.parameter_position(stack));
                }
            }
            for body in statement_blocks(statement) {
                self.summarize(body, summary);
            }
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut checks = Checks {
            analysis: self,
            blocks: Blocks::of(self.function),
            diagnostics: Vec::new(),
            report: true,
            loops: Vec::new(),
            iterated: Vec::new(),
        };
        checks.escapes(&self.function.body);
        checks.block(&self.function.body, Live::new());
        checks.diagnostics
    }
}

/// The bindings a call passes with `variable` to the parameters at `positions`, each with where it is passed.
fn passed_at(call: &Call, positions: &BTreeSet<usize>) -> Vec<(LocalId, Span)> {
    call.arguments
        .iter()
        .enumerate()
        .filter_map(|(position, argument)| match *argument {
            Argument::Reference { local, span } if positions.contains(&position) => {
                Some((local, span))
            }
            Argument::Reference { .. } | Argument::Value(_) => None,
        })
        .collect()
}

/// Which block declares each local, and which block holds each block. The function's body is block 0.
struct Blocks {
    declared_in: HashMap<LocalId, usize>,
    parents: Vec<Option<usize>>,
}

impl Blocks {
    fn of(function: &Function<'_>) -> Self {
        let mut blocks = Self {
            declared_in: function
                .parameters
                .iter()
                .map(|&local| (local, 0))
                .collect(),
            parents: vec![None],
        };
        blocks.add(&function.body, 0);
        blocks
    }

    fn add(&mut self, block: &Block, id: usize) {
        for statement in &block.statements {
            if let Statement::Declare { local, .. } = statement {
                self.declared_in.insert(*local, id);
            }
            // A loop's variable and a case's bindings belong to the block they run in, and end with it.
            let declared: Vec<Vec<LocalId>> = match statement {
                Statement::ForRange { variable, .. } | Statement::ForEach { variable, .. } => {
                    vec![variable.iter().copied().collect()]
                }
                Statement::Match { cases, .. } => cases
                    .iter()
                    .map(|case| case.bindings.iter().map(|binding| binding.local).collect())
                    .collect(),
                _ => Vec::new(),
            };
            for (position, body) in statement_blocks(statement).into_iter().enumerate() {
                let child = self.parents.len();
                self.parents.push(Some(id));
                for &local in declared.get(position).into_iter().flatten() {
                    self.declared_in.insert(local, child);
                }
                self.add(body, child);
            }
        }
    }

    /// Whether `inner` is `outer` or lies inside it.
    fn within(&self, mut inner: usize, outer: usize) -> bool {
        loop {
            if inner == outer {
                return true;
            }
            match self.parents.get(inner).copied().flatten() {
                Some(parent) => inner = parent,
                None => return false,
            }
        }
    }
}

/// The views some later statement reads.
type Live = BTreeSet<LocalId>;

/// Where `break` and `continue` lead in the innermost loop: what is live after it, and at its head.
#[derive(Clone)]
struct LoopEnds {
    exit: Live,
    head: Live,
}

/// The checks of one function.
struct Checks<'a, 'b, 'src> {
    analysis: &'b Analysis<'a, 'src>,
    blocks: Blocks,
    diagnostics: Vec<Diagnostic>,
    /// False while a loop is worked out to a fixed point, so its body reports only once.
    report: bool,
    loops: Vec<LoopEnds>,
    /// The roots of what the `for` loops around the current point go over, with where each names it.
    iterated: Vec<(Roots, Span)>,
}

impl Checks<'_, '_, '_> {
    fn function(&self) -> &Function<'_> {
        self.analysis.function
    }

    fn name(&self, local: LocalId) -> &str {
        self.analysis.function.locals[local].name
    }

    /// L0701: walks the function for views that would outlive their roots.
    fn escapes(&mut self, block: &Block) {
        for statement in &block.statements {
            match statement {
                Statement::Declare { local, value } if is_view(self.function(), *local) => {
                    self.held(*local, value);
                }
                Statement::Assign { target, value }
                    if target.steps.is_empty() && is_view(self.function(), target.local) =>
                {
                    self.held(target.local, value);
                }
                Statement::Return(Some(value)) if value.ty.as_view().is_some() => {
                    self.returned(value);
                }
                _ => {}
            }
            let mut calls = Vec::new();
            statement_calls(statement, &mut calls);
            for call in calls {
                for (view, roots, span) in self.analysis.taken(call) {
                    self.check_held(view, &roots, span);
                }
            }
            for body in statement_blocks(statement) {
                self.escapes(body);
            }
        }
    }

    /// Checks that `view` can hold a view of `value`.
    fn held(&mut self, view: LocalId, value: &Expr) {
        let roots = self.analysis.roots_of(value);
        self.check_held(view, &roots, value.span);
    }

    /// Checks that `view` outlasts none of `roots`, which reach it at `span`.
    fn check_held(&mut self, view: LocalId, roots: &Roots, span: Span) {
        let parameter = self.analysis.parameter_position(view).is_some();
        for root in roots {
            let diagnostic = match *root {
                Root::Static => continue,
                Root::Temporary => errors::view_of_temporary(self.name(view), span),
                Root::Local(local) if self.analysis.parameter_position(local).is_some() => continue,
                // The caller keeps a `variable` view parameter after the call, when every binding here is gone.
                Root::Local(local) if parameter => errors::view_escapes_call(
                    self.name(view),
                    self.name(local),
                    self.function().name,
                    span,
                ),
                Root::Local(local) => {
                    let root_block = self.blocks.declared_in.get(&local).copied().unwrap_or(0);
                    let view_block = self.blocks.declared_in.get(&view).copied().unwrap_or(0);
                    if self.blocks.within(view_block, root_block) {
                        continue;
                    }
                    let declared = self.function().locals[local].span;
                    errors::view_outlives_root(self.name(view), self.name(local), span, declared)
                }
            };
            self.diagnostics.push(diagnostic);
            return;
        }
    }

    /// Checks that a returned view shows nothing the function frees.
    fn returned(&mut self, value: &Expr) {
        for root in self.analysis.roots_of(value) {
            let diagnostic = match root {
                Root::Static => continue,
                Root::Temporary => errors::returned_view_of_temporary(value.span),
                Root::Local(local) if self.analysis.parameter_position(local).is_some() => continue,
                Root::Local(local) => errors::returned_view_of_local(
                    self.name(local),
                    self.function().name,
                    value.span,
                ),
            };
            self.diagnostics.push(diagnostic);
            return;
        }
    }

    /**
     * L0702 and L0703: walks `block` backward from `live`, the views read
     * after it, and returns the views read from its start on. A list that
     * frees elements at a point where a view of it is live is the error.
     */
    fn block(&mut self, block: &Block, live: Live) -> Live {
        let mut live = live;
        for statement in block.statements.iter().rev() {
            live = self.statement(statement, live);
        }
        live
    }

    /**
     * The views live before `statement`, given those live after it. A pop
     * or a call in the statement's own expressions runs before the blocks it
     * holds, so what it could free must not be read there, by the statement
     * itself or after it.
     */
    fn statement(&mut self, statement: &Statement, after: Live) -> Live {
        let mut read = Live::new();
        statement_reads(self.function(), statement, &mut read);
        let before = self.live_before(statement, after.clone(), read);
        let in_use: Live = before.union(&after).copied().collect();
        for (stack, span) in statement_pops(statement) {
            self.freed(stack, Freeing::Popped(span), &in_use);
        }
        let mut calls = Vec::new();
        statement_calls(statement, &mut calls);
        for call in calls {
            let callee = &self.analysis.summaries[call.function];
            let name = self.analysis.functions[call.function].name;
            for (position, argument) in call.arguments.iter().enumerate() {
                let Argument::Reference { local, span } = *argument else {
                    continue;
                };
                // A call that can do both is reported once, for the replacement, which frees more.
                let how = if callee.replaces.contains(&position) {
                    Freeing::PassedToReplace(span, name)
                } else if callee.pops.contains(&position) {
                    Freeing::PassedToPop(span, name)
                } else {
                    continue;
                };
                self.freed(local, how, &in_use);
            }
        }
        before
    }

    /// The views live before `statement`, from those live `after` it and those it `read`s outside its blocks.
    fn live_before(&mut self, statement: &Statement, after: Live, read: Live) -> Live {
        match statement {
            Statement::Declare { local, .. } => {
                let mut live = after;
                live.remove(local);
                live.extend(read);
                live
            }
            Statement::Assign { target, .. } => {
                if target.steps.is_empty() && is_list(self.function(), target.local) {
                    self.freed(target.local, Freeing::Assigned(target.span), &after);
                }
                let mut live = after;
                if target.steps.is_empty() {
                    live.remove(&target.local);
                }
                live.extend(read);
                live
            }
            Statement::Return(_) => read,
            Statement::Break => self.loops.last().map_or(read, |ends| ends.exit.clone()),
            Statement::Continue => self.loops.last().map_or(read, |ends| ends.head.clone()),
            Statement::If {
                branches,
                otherwise,
            } => {
                let mut live = match otherwise {
                    Some(block) => self.block(block, after.clone()),
                    None => after.clone(),
                };
                for branch in branches.iter().rev() {
                    let body = self.block(&branch.body, after.clone());
                    live.extend(body);
                }
                live.extend(read);
                live
            }
            Statement::While { body, .. } => {
                // The condition is read at the head of every iteration, and once more where it ends the loop.
                let mut entry = after.clone();
                entry.extend(read.iter().copied());
                let mut head = self.repeat(body, &after, entry);
                head.extend(read);
                head
            }
            Statement::ForRange { body, .. } => {
                let mut head = self.repeat(body, &after, after.clone());
                head.extend(read);
                head
            }
            Statement::ForEach { array, body, .. } => {
                let roots = self.analysis.roots_of(array);
                self.iterated.push((roots, array.span));
                let mut head = self.repeat(body, &after, after.clone());
                self.iterated.pop();
                head.extend(read);
                head
            }
            Statement::Match { cases, .. } => {
                let mut live = after.clone();
                for case in cases {
                    live.extend(self.block(&case.body, after.clone()));
                }
                live.extend(read);
                live
            }
            Statement::Discard(_)
            | Statement::Call(_)
            | Statement::Push { .. }
            | Statement::Print(_)
            | Statement::Assert { .. } => {
                let mut live = after;
                live.extend(read);
                live
            }
        }
    }

    /**
     * The views live at the head of a loop whose body is `body`: worked out
     * to a fixed point, starting from `start`, the views read after the
     * loop and at its head. Then the body is walked once more to report.
     */
    fn repeat(&mut self, body: &Block, after: &Live, start: Live) -> Live {
        let report = std::mem::replace(&mut self.report, false);
        let mut head = start.clone();
        loop {
            self.loops.push(LoopEnds {
                exit: after.clone(),
                head: head.clone(),
            });
            let mut next = start.clone();
            next.extend(self.block(body, head.clone()));
            self.loops.pop();
            if next == head {
                break;
            }
            head = next;
        }
        self.report = report;
        if report {
            self.loops.push(LoopEnds {
                exit: after.clone(),
                head: head.clone(),
            });
            self.block(body, head.clone());
            self.loops.pop();
        }
        head
    }

    /// Reports `list`, which frees elements as `how` says, if a `for` loop goes over it or a view of it is in use.
    fn freed(&mut self, list: LocalId, how: Freeing<'_>, in_use: &Live) {
        if !self.report {
            return;
        }
        let root = Root::Local(list);
        let kind = self.function().locals[list]
            .ty
            .as_list()
            .map_or(Sequence::List, |list| list.kind);
        let looped = self
            .iterated
            .iter()
            .rev()
            .find(|(roots, _)| roots.contains(&root));
        if let Some(&(_, loop_span)) = looped {
            let diagnostic = errors::freed_while_iterated(self.name(list), kind, how, loop_span);
            self.diagnostics.push(diagnostic);
            return;
        }
        let viewing = in_use.iter().copied().find(|&view| {
            self.analysis
                .roots
                .get(&view)
                .is_some_and(|roots| roots.contains(&root))
        });
        if let Some(view) = viewing {
            let declared = self.function().locals[view].span;
            let diagnostic =
                errors::freed_while_viewed(self.name(list), kind, self.name(view), how, declared);
            self.diagnostics.push(diagnostic);
        }
    }
}

/// The views a statement reads outside the blocks it holds.
fn statement_reads(function: &Function<'_>, statement: &Statement, read: &mut Live) {
    let mut values: Vec<&Expr> = Vec::new();
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
        | Statement::Match { value, .. } => values.push(value),
        Statement::Assign { target, value } => {
            for step in &target.steps {
                if let PlaceStep::Index(index) = step {
                    values.push(&index.value);
                }
            }
            values.push(value);
        }
        Statement::Call(call) => call_reads(function, call, read),
        Statement::Print(parts) => {
            for part in parts {
                if let PrintPart::Value { value, .. } = part {
                    values.push(value);
                }
            }
        }
        Statement::If { branches, .. } => {
            values.extend(branches.iter().map(|branch| &branch.condition));
        }
        Statement::ForRange { start, end, .. } => {
            values.push(start);
            values.push(end);
        }
        Statement::Return(None) | Statement::Break | Statement::Continue => {}
    }
    for value in values {
        expr_reads(function, value, read);
    }
}

fn call_reads(function: &Function<'_>, call: &Call, read: &mut Live) {
    for argument in &call.arguments {
        match argument {
            Argument::Value(value) => expr_reads(function, value, read),
            Argument::Reference { local, .. } => {
                if is_view(function, *local) {
                    read.insert(*local);
                }
            }
        }
    }
}

/// The views an expression reads.
fn expr_reads(function: &Function<'_>, expr: &Expr, read: &mut Live) {
    match &expr.kind {
        ExprKind::Local(local) => {
            if is_view(function, *local) {
                read.insert(*local);
            }
        }
        ExprKind::Literal(_) | ExprKind::Constant(_) | ExprKind::EmptyList | ExprKind::Pop(_) => {}
        ExprKind::Negate { operand, .. }
        | ExprKind::Not(operand)
        | ExprKind::Convert { operand, .. }
        | ExprKind::Fill(operand)
        | ExprKind::Length(operand)
        | ExprKind::View(operand)
        | ExprKind::Bytes(operand)
        | ExprKind::Top(operand)
        | ExprKind::Field {
            object: operand, ..
        } => expr_reads(function, operand, read),
        ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => {
            for field in fields {
                expr_reads(function, &field.value, read);
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
            expr_reads(function, left, read);
            expr_reads(function, right, read);
        }
        ExprKind::Slice { items, start, end } => {
            expr_reads(function, items, read);
            expr_reads(function, start, read);
            expr_reads(function, end, read);
        }
        ExprKind::Call(call) => call_reads(function, call, read),
    }
}
