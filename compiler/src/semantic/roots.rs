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
 *   A pool is replaced while something that holds handles into it is still
 *   used, or a `for` loop goes over handles into it: the handles would name
 *   the slots of the new pool, which a generation check cannot tell apart.
 */

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::errors::{self, Freeing, Holder, Owner};
use super::handles::PoolsOfLocals;
use super::walk::{Blocks, statement_calls, statement_pops};
use crate::diagnostics::Diagnostic;
use crate::ir::walk::{each_expr, statement_blocks};
use crate::ir::{
    Argument, Block, Call, Expr, ExprKind, Function, LocalId, Place, PlaceStep, PrintPart,
    Statement, text_values,
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

/**
 * Every error L0701, L0702 and L0703 in the program, in source order.
 * `pools` gives, for each function, the pools each local can hold handles
 * into.
 */
pub(super) fn check(functions: &[Function<'_>], pools: &PoolsOfLocals) -> Vec<Diagnostic> {
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
        .zip(pools)
        .flat_map(|(function, pools)| {
            Analysis::of(function, functions, &summaries).diagnostics(pools)
        })
        .collect();
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.span.start);
    diagnostics
}

fn is_view(function: &Function<'_>, local: LocalId) -> bool {
    function.locals[local].ty.as_view().is_some()
}

/// Whether replacing `local` frees what it holds: a list, a stack, a pool, or text.
fn frees_on_replace(function: &Function<'_>, local: LocalId) -> bool {
    let ty = &function.locals[local].ty;
    ty.is_growable() || (ty.holds_text() && ty.as_view().is_none())
}

/**
 * Whether the assignment of `value` to `target` frees what something may
 * still read: a whole list, stack, pool or text replaced, or new text in a
 * part of a binding that holds text, which gathers that binding's text again.
 */
fn assignment_frees(function: &Function<'_>, target: &Place, value: &Expr) -> bool {
    frees_on_replace(function, target.local) && (target.steps.is_empty() || value.ty.holds_text())
}

/// Whether later statements reading `local` matter to these checks: a view, or a value that holds handles.
fn is_tracked(function: &Function<'_>, local: LocalId) -> bool {
    let ty = &function.locals[local].ty;
    ty.as_view().is_some() || ty.holds_handle()
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

    /**
     * The roots of a view made from `expr`: what keeps the elements it would
     * show, or the bytes of the text it is. Text in the program itself lasts
     * as long as the program; a binding keeps its own text.
     */
    fn roots_of(&self, expr: &Expr) -> Roots {
        match &expr.kind {
            ExprKind::Local(local) => self.local_roots(*local),
            ExprKind::Field { object: inner, .. }
            | ExprKind::Index { array: inner, .. }
            | ExprKind::View(inner)
            | ExprKind::Bytes(inner)
            | ExprKind::Slice { items: inner, .. } => self.roots_of(inner),
            ExprKind::Literal(_) | ExprKind::Constant(_) => Roots::from([Root::Static]),
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
                Statement::Assign { target, value }
                    if assignment_frees(self.function, target, value) =>
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

    fn diagnostics(&self, pools: &HashMap<LocalId, BTreeSet<LocalId>>) -> Vec<Diagnostic> {
        let mut checks = Checks {
            analysis: self,
            pools,
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

/// The views, and the values that hold handles, some later statement reads.
type Live = BTreeSet<LocalId>;

/// What a `for` loop around the current point goes over.
struct Iterated {
    /// The roots of the elements, if they belong to something else.
    roots: Roots,
    /// The pools its elements can hold handles into.
    pools: BTreeSet<LocalId>,
    /// Where the loop names it.
    span: Span,
}

/// Where `break` and `continue` lead in the innermost loop: what is live after it, and at its head.
#[derive(Clone)]
struct LoopEnds {
    exit: Live,
    head: Live,
}

/// The checks of one function.
struct Checks<'a, 'b, 'src> {
    analysis: &'b Analysis<'a, 'src>,
    /// The pools each local can hold handles into.
    pools: &'b HashMap<LocalId, BTreeSet<LocalId>>,
    blocks: Blocks,
    diagnostics: Vec<Diagnostic>,
    /// False while a loop is worked out to a fixed point, so its body reports only once.
    report: bool,
    loops: Vec<LoopEnds>,
    /// What the `for` loops around the current point go over, innermost last.
    iterated: Vec<Iterated>,
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
            Statement::Assign { target, value } => {
                if assignment_frees(self.function(), target, value) {
                    let how = if target.steps.is_empty() {
                        Freeing::Assigned(target.span)
                    } else {
                        Freeing::NewTextInPart(target.span)
                    };
                    self.freed(target.local, how, &after);
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
                let mut pools = BTreeSet::new();
                each_expr(array, &mut |inner| {
                    if let ExprKind::Local(local) = inner.kind {
                        pools.extend(self.pools.get(&local).into_iter().flatten().copied());
                    }
                });
                self.iterated.push(Iterated {
                    roots,
                    pools,
                    span: array.span,
                });
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
            // A `try` that fails leaves the function, where nothing is read any more.
            Statement::Discard(_)
            | Statement::Call(_)
            | Statement::Try(_)
            | Statement::Push { .. }
            | Statement::Remove { .. }
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

    /**
     * Reports `local`, a list, a stack, a pool or a value that holds text,
     * which frees what it keeps as `how` says, if a `for` loop goes over it
     * or a view of it is in use.
     */
    fn freed(&mut self, local: LocalId, how: Freeing<'_>, in_use: &Live) {
        if !self.report {
            return;
        }
        let ty = &self.function().locals[local].ty;
        if ty.as_pool().is_some() {
            self.pool_freed(local, how, in_use);
            return;
        }
        let owner = ty
            .as_list()
            .map_or(Owner::Text, |list| Owner::Sequence(list.kind));
        let root = Root::Local(local);
        let looped = self
            .iterated
            .iter()
            .rev()
            .find(|iterated| iterated.roots.contains(&root));
        if let Some(iterated) = looped {
            let diagnostic =
                errors::freed_while_iterated(self.name(local), owner, how, iterated.span);
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
                errors::freed_while_viewed(self.name(local), owner, self.name(view), how, declared);
            self.diagnostics.push(diagnostic);
        }
    }

    /// Reports `pool`, replaced as `how` says, if a `for` loop goes over handles into it or something in use holds one.
    fn pool_freed(&mut self, pool: LocalId, how: Freeing<'_>, in_use: &Live) {
        let looped = self
            .iterated
            .iter()
            .rev()
            .find(|iterated| iterated.pools.contains(&pool));
        let holder = match looped {
            Some(iterated) => Some(Holder::Loop(iterated.span)),
            None => in_use
                .iter()
                .copied()
                .find(|&local| {
                    local != pool
                        && self
                            .pools
                            .get(&local)
                            .is_some_and(|pools| pools.contains(&pool))
                })
                .map(|local| {
                    let declared = self.function().locals[local].span;
                    Holder::Binding(self.name(local), declared)
                }),
        };
        if let Some(holder) = holder {
            let diagnostic = errors::pool_replaced_while_handled(self.name(pool), how, holder);
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
        | Statement::Remove { handle: value, .. }
        | Statement::Return(Some(value))
        | Statement::Assert { condition: value }
        | Statement::While {
            condition: value, ..
        }
        | Statement::ForEach { array: value, .. }
        | Statement::Match { value, .. } => values.push(value),
        Statement::Assign { target, value } => {
            for step in &target.steps {
                match step {
                    PlaceStep::Index(index) => values.push(&index.value),
                    PlaceStep::Slot(slot) => values.push(&slot.handle),
                    PlaceStep::Field(_) => {}
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
        Statement::Try(attempt) => values.extend(attempt.parts()),
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
                if is_tracked(function, *local) {
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
            if is_tracked(function, *local) {
                read.insert(*local);
            }
        }
        ExprKind::Literal(_) | ExprKind::Constant(_) | ExprKind::EmptyList => {}
        ExprKind::Pop(stack) => {
            if is_tracked(function, *stack) {
                read.insert(*stack);
            }
        }
        ExprKind::Negate { operand, .. }
        | ExprKind::Not(operand)
        | ExprKind::Convert { operand, .. }
        | ExprKind::Fill(operand)
        | ExprKind::Length(operand)
        | ExprKind::View(operand)
        | ExprKind::Bytes(operand)
        | ExprKind::Top(operand)
        | ExprKind::Insert { value: operand, .. }
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
        | ExprKind::Bits { left, right, .. }
        | ExprKind::Index {
            array: left,
            index: right,
            ..
        }
        | ExprKind::Slot {
            pool: left,
            handle: right,
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
        ExprKind::Text(parts) => {
            for value in text_values(parts) {
                expr_reads(function, value, read);
            }
        }
        ExprKind::Try(attempt) => {
            for part in attempt.parts() {
                expr_reads(function, part, read);
            }
        }
        ExprKind::System { arguments, .. } => {
            for argument in arguments {
                expr_reads(function, argument, read);
            }
        }
    }
}
