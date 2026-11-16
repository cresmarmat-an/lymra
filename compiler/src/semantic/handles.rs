/*!
 * Which pools the handles of every value can come from, and the errors that
 * keep a handle from being used with another pool or from outliving its
 * pool (spec 7.6).
 *
 * A handle comes from the pool whose `insert` made it. The pass follows
 * handles from there through bindings, lists, the fields of other values,
 * the values a pool holds, and calls, to a fixed point across the program.
 * The handles in a pool's own values count as handles into the pool itself,
 * so a copy of the pool, or one a function returns, keeps handles into
 * itself. A function that gets handles from its caller, in a parameter or in
 * the values of a pool parameter, cannot tell which pool they come from;
 * using them with a pool parameter requires that pool, and every call is
 * checked against what its function requires.
 *
 * - L0705: a handle is used with a pool it may not come from.
 * - L0701: handles are kept after their pool is freed: by a binding of an
 *   outer block, by a returned value, or by a `variable` parameter.
 */

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::errors::{self, Origin};
use super::walk::{Blocks, each_expr, statement_blocks, statement_calls, statement_exprs};
use crate::diagnostics::Diagnostic;
use crate::ir::{Argument, Block, Call, Expr, ExprKind, Function, LocalId, PlaceStep, Statement};
use crate::source::Span;

/// A pool that handles can come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    /// The pool a local holds: a pool binding of the function, or a pool parameter.
    Pool(LocalId),
    /// Whichever pools the handles a parameter brings from the caller come from, in it or in the values of a pool.
    Within(LocalId),
    /// The pool that holds the value these handles are in. Only the values of a pool count handles this way.
    Itself,
    /// A pool computed without a binding, which keeps no handle.
    Temporary,
}

type Sources = BTreeSet<Source>;

/// Where handles can come from, as a function's callers see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Passed {
    /// The pool passed at the position.
    Pool(usize),
    /// The pools that the handles passed at the position come from.
    Within(usize),
    /// The pool the function returns, for the handles in its own values.
    Itself,
}

/// What a function does with handles, as its callers see it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Summary {
    /// Where the handles in the value it returns can come from.
    returns: BTreeSet<Passed>,
    /// For each `variable` parameter that holds handles, by position, where the handles it gets can come from.
    takes: BTreeMap<usize, BTreeSet<Passed>>,
    /// The handles it uses with a pool parameter: where they come from, and the position of the pool.
    requires: BTreeSet<(Passed, usize)>,
}

/// For each function, the pools each of its locals that hold handles can hold handles into.
pub(super) type PoolsOfLocals = Vec<HashMap<LocalId, BTreeSet<LocalId>>>;

/**
 * Every error L0705, and every L0701 about handles, in the program, and for
 * each function the pools each local can hold handles into. A handle a
 * parameter brings counts as one into every pool parameter.
 */
pub(super) fn check(functions: &[Function<'_>]) -> (Vec<Diagnostic>, PoolsOfLocals) {
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
    let mut diagnostics = Vec::new();
    let mut pools = Vec::with_capacity(functions.len());
    for function in functions {
        let analysis = Analysis::of(function, functions, &summaries);
        diagnostics.extend(analysis.diagnostics());
        pools.push(analysis.pools_of_locals());
    }
    (diagnostics, pools)
}

/// What the handles of one function's values can come from.
struct Analysis<'a, 'src> {
    function: &'a Function<'src>,
    functions: &'a [Function<'src>],
    summaries: &'a [Summary],
    sources: HashMap<LocalId, Sources>,
}

impl<'a, 'src> Analysis<'a, 'src> {
    /// Follows the handles of `function` until nothing more arrives anywhere.
    fn of(
        function: &'a Function<'src>,
        functions: &'a [Function<'src>],
        summaries: &'a [Summary],
    ) -> Self {
        let mut analysis = Self {
            function,
            functions,
            summaries,
            sources: HashMap::new(),
        };
        for &parameter in &function.parameters {
            if function.locals[parameter].ty.holds_handle() {
                analysis
                    .sources
                    .insert(parameter, Sources::from([Source::Within(parameter)]));
            }
        }
        loop {
            let before = analysis.sources.clone();
            analysis.gather(&function.body);
            if analysis.sources == before {
                break;
            }
        }
        analysis
    }

    fn gather(&mut self, block: &Block) {
        for statement in &block.statements {
            for (local, value) in receivers(statement) {
                let sources = self.sources_of(value);
                self.give(local, sources);
            }
            if let Statement::Match { value, cases } = statement {
                let sources = self.sources_of(value);
                for binding in cases.iter().flat_map(|case| &case.bindings) {
                    self.give(binding.local, sources.clone());
                }
            }
            let mut calls = Vec::new();
            statement_calls(statement, &mut calls);
            for call in calls {
                for (local, sources, _) in self.taken(call) {
                    self.give(local, sources);
                }
            }
            for body in statement_blocks(statement) {
                self.gather(body);
            }
        }
    }

    /// Adds `sources` to where the handles in `local` can come from. In a pool's values, its own handles count as `Itself`.
    fn give(&mut self, local: LocalId, sources: Sources) {
        if !self.function.locals[local].ty.holds_handle() || sources.is_empty() {
            return;
        }
        let pool = self.function.locals[local].ty.as_pool().is_some();
        let entry = self.sources.entry(local).or_default();
        entry.extend(sources.into_iter().map(|source| {
            if pool && source == Source::Pool(local) {
                Source::Itself
            } else {
                source
            }
        }));
    }

    /**
     * The handles a call gives its `variable` parameters that hold handles:
     * each binding passed there, where the handles can come from, and where
     * it is passed.
     */
    fn taken(&self, call: &Call) -> Vec<(LocalId, Sources, Span)> {
        self.summaries[call.function]
            .takes
            .iter()
            .filter_map(|(&position, passed)| {
                let Some(&Argument::Reference { local, span }) = call.arguments.get(position)
                else {
                    return None;
                };
                let sources = passed
                    .iter()
                    .flat_map(|&passed| self.mapped(call, passed))
                    .collect();
                Some((local, sources, span))
            })
            .collect()
    }

    /// Where the handles in the value of `expr` can come from. For a pool, those in its values, counted from it.
    fn sources_of(&self, expr: &Expr) -> Sources {
        if !expr.ty.holds_handle() {
            return Sources::new();
        }
        match &expr.kind {
            ExprKind::Local(local) | ExprKind::Pop(local) => {
                self.sources.get(local).cloned().unwrap_or_default()
            }
            ExprKind::Insert { pool, .. } => Sources::from([Source::Pool(*pool)]),
            ExprKind::Slot { pool, .. } => self.absolute(self.sources_of(pool), identity(pool)),
            ExprKind::Index { array: inner, .. }
            | ExprKind::Field { object: inner, .. }
            | ExprKind::View(inner)
            | ExprKind::Slice { items: inner, .. }
            | ExprKind::Top(inner)
            | ExprKind::Fill(inner) => self.sources_of(inner),
            ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => fields
                .iter()
                .flat_map(|field| self.sources_of(&field.value))
                .collect(),
            ExprKind::Call(call) => self.summaries[call.function]
                .returns
                .iter()
                .flat_map(|&passed| self.mapped(call, passed))
                .collect(),
            ExprKind::Literal(_)
            | ExprKind::Constant(_)
            | ExprKind::Negate { .. }
            | ExprKind::Not(_)
            | ExprKind::Arithmetic { .. }
            | ExprKind::Compare { .. }
            | ExprKind::Logic { .. }
            | ExprKind::Convert { .. }
            | ExprKind::Wrapping { .. }
            | ExprKind::EmptyList
            | ExprKind::Bytes(_)
            | ExprKind::Text(_)
            | ExprKind::Length(_) => Sources::new(),
        }
    }

    /// `sources`, the handles in a pool's values, with those into the pool itself named as `pool`.
    fn absolute(&self, sources: Sources, pool: Source) -> Sources {
        sources
            .into_iter()
            .map(|source| {
                if source == Source::Itself {
                    pool
                } else {
                    source
                }
            })
            .collect()
    }

    /// Where the handles that `passed` names come from, at this call.
    fn mapped(&self, call: &Call, passed: Passed) -> Sources {
        match passed {
            Passed::Pool(position) => Sources::from([argument_identity(call, position)]),
            Passed::Within(position) => match call.arguments.get(position) {
                Some(Argument::Reference { local, .. }) => self.absolute(
                    self.sources.get(local).cloned().unwrap_or_default(),
                    Source::Pool(*local),
                ),
                Some(Argument::Value(value)) => {
                    self.absolute(self.sources_of(value), identity(value))
                }
                None => Sources::new(),
            },
            Passed::Itself => Sources::from([Source::Itself]),
        }
    }

    fn parameter_position(&self, local: LocalId) -> Option<usize> {
        self.function
            .parameters
            .iter()
            .position(|&parameter| parameter == local)
    }

    /// `source` as the function's callers see it, or `None` for a pool only the function knows.
    fn as_passed(&self, source: Source) -> Option<Passed> {
        match source {
            Source::Pool(local) => self.parameter_position(local).map(Passed::Pool),
            Source::Within(local) => self.parameter_position(local).map(Passed::Within),
            Source::Itself => Some(Passed::Itself),
            Source::Temporary => None,
        }
    }

    fn summary(&self) -> Summary {
        let mut checks = Checks::new(self);
        checks.block(&self.function.body);
        let mut summary = Summary {
            requires: checks.requires,
            ..Summary::default()
        };
        for value in returned(&self.function.body) {
            let sources = self.sources_of(value);
            summary.returns.extend(
                sources
                    .into_iter()
                    .filter_map(|source| self.as_passed(source)),
            );
        }
        for (position, &parameter) in self.function.parameters.iter().enumerate() {
            let local = &self.function.locals[parameter];
            if local.by_reference && local.ty.holds_handle() {
                let passed = self.sources.get(&parameter).into_iter().flatten();
                let passed = passed
                    .filter_map(|&source| self.as_passed(source))
                    .collect();
                summary.takes.insert(position, passed);
            }
        }
        summary
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut checks = Checks::new(self);
        checks.report = true;
        checks.block(&self.function.body);
        checks.diagnostics
    }

    /// For each local that holds handles, the pools it can hold handles into; one a parameter brings, into every pool parameter.
    fn pools_of_locals(&self) -> HashMap<LocalId, BTreeSet<LocalId>> {
        let pool_parameters: Vec<LocalId> = self
            .function
            .parameters
            .iter()
            .copied()
            .filter(|&parameter| self.function.locals[parameter].ty.as_pool().is_some())
            .collect();
        self.sources
            .iter()
            .map(|(&local, sources)| {
                let pools = sources
                    .iter()
                    .flat_map(|&source| match source {
                        Source::Pool(pool) => vec![pool],
                        Source::Within(_) => pool_parameters.clone(),
                        Source::Itself | Source::Temporary => Vec::new(),
                    })
                    .collect();
                (local, pools)
            })
            .collect()
    }
}

/// The pool `expr`, a value of a pool type, is.
fn identity(expr: &Expr) -> Source {
    match &expr.kind {
        ExprKind::Local(local) => Source::Pool(*local),
        _ => Source::Temporary,
    }
}

/// The pool a call passes at `position`.
fn argument_identity(call: &Call, position: usize) -> Source {
    match call.arguments.get(position) {
        Some(Argument::Reference { local, .. }) => Source::Pool(*local),
        Some(Argument::Value(value)) => identity(value),
        None => Source::Temporary,
    }
}

/**
 * The locals a statement gives a value to, outside the blocks it holds,
 * with the value: a binding, an assigned local or a part of it, a list it
 * pushes to, a pool it inserts into, and a `for` loop's variable.
 */
fn receivers(statement: &Statement) -> Vec<(LocalId, &Expr)> {
    let mut receivers = Vec::new();
    match statement {
        Statement::Declare { local, value } => receivers.push((*local, value)),
        Statement::Assign { target, value } => receivers.push((target.local, value)),
        Statement::Push { list, value, .. } => receivers.push((*list, value)),
        Statement::ForEach {
            variable: Some(variable),
            array,
            ..
        } => receivers.push((*variable, array)),
        _ => {}
    }
    for expr in statement_exprs(statement) {
        each_expr(expr, &mut |inner| {
            if let ExprKind::Insert { pool, value } = &inner.kind {
                receivers.push((*pool, value.as_ref()));
            }
        });
    }
    receivers
}

/// The values `block` and the blocks inside it return.
fn returned(block: &Block) -> Vec<&Expr> {
    let mut values = Vec::new();
    for statement in &block.statements {
        if let Statement::Return(Some(value)) = statement {
            values.push(value);
        }
        for body in statement_blocks(statement) {
            values.extend(returned(body));
        }
    }
    values
}

/// Whether handles from a source can be used with a pool.
enum Verdict {
    Holds,
    /// They can, if the caller passes handles from that pool: where the handles come from, and the pool's position.
    Requires(Passed, usize),
    Fails,
}

/// The checks of one function, and what it requires of its callers.
struct Checks<'a, 'b, 'src> {
    analysis: &'b Analysis<'a, 'src>,
    blocks: Blocks,
    requires: BTreeSet<(Passed, usize)>,
    /// Whether errors are kept, once the summaries are worked out.
    report: bool,
    diagnostics: Vec<Diagnostic>,
}

impl<'a, 'b, 'src> Checks<'a, 'b, 'src> {
    fn new(analysis: &'b Analysis<'a, 'src>) -> Self {
        Self {
            analysis,
            blocks: Blocks::of(analysis.function),
            requires: BTreeSet::new(),
            report: false,
            diagnostics: Vec::new(),
        }
    }

    fn function(&self) -> &Function<'src> {
        self.analysis.function
    }

    fn block(&mut self, block: &Block) {
        for statement in &block.statements {
            self.statement(statement);
            for body in statement_blocks(statement) {
                self.block(body);
            }
        }
    }

    fn statement(&mut self, statement: &Statement) {
        for expr in statement_exprs(statement) {
            let mut slots = Vec::new();
            each_expr(expr, &mut |inner| {
                if let ExprKind::Slot { pool, handle } = &inner.kind {
                    slots.push((identity(pool), handle.as_ref()));
                }
            });
            for (pool, handle) in slots {
                self.used(handle, pool);
            }
        }
        match statement {
            Statement::Remove { pool, handle, .. } => self.used(handle, Source::Pool(*pool)),
            Statement::Assign { target, .. } => {
                if let Some(PlaceStep::Slot(slot)) = target.steps.first() {
                    self.used(&slot.handle, Source::Pool(target.local));
                }
            }
            Statement::Return(Some(value)) => self.returns(value),
            _ => {}
        }
        for (local, value) in receivers(statement) {
            let sources = self.analysis.sources_of(value);
            self.kept(local, &sources, value.span);
        }
        let mut calls = Vec::new();
        statement_calls(statement, &mut calls);
        for call in calls {
            self.call(call);
        }
    }

    /// Checks the handle `handle` used with the pool `pool`.
    fn used(&mut self, handle: &Expr, pool: Source) {
        for source in self.analysis.sources_of(handle) {
            match self.verdict(source, pool) {
                Verdict::Holds => {}
                Verdict::Requires(passed, position) => {
                    self.requires.insert((passed, position));
                }
                Verdict::Fails => {
                    let diagnostic = errors::handle_from_another_pool(
                        handle.span,
                        &self.pool_name(pool),
                        &self.origin(source),
                    );
                    self.report(diagnostic);
                    return;
                }
            }
        }
    }

    /// Checks a call against what its function requires of the handles and pools it gets.
    fn call(&mut self, call: &Call) {
        let analysis = self.analysis;
        for &(passed, position) in &analysis.summaries[call.function].requires {
            let pool = argument_identity(call, position);
            for source in analysis.mapped(call, passed) {
                match self.verdict(source, pool) {
                    Verdict::Holds => {}
                    Verdict::Requires(passed, position) => {
                        self.requires.insert((passed, position));
                    }
                    Verdict::Fails => {
                        if let Some(span) = argument_span(call, passed) {
                            let diagnostic = errors::call_mixes_pools(
                                span,
                                analysis.functions[call.function].name,
                                &self.pool_name(pool),
                                &self.origin(source),
                            );
                            self.report(diagnostic);
                        }
                        break;
                    }
                }
            }
        }
        for (local, sources, span) in analysis.taken(call) {
            self.kept(local, &sources, span);
        }
    }

    /**
     * Whether handles from `source` can be used with `pool`. Inside a
     * function, a pool parameter can be the pool of any handle the caller
     * passes, so the function requires that of its callers.
     */
    fn verdict(&self, source: Source, pool: Source) -> Verdict {
        if source == pool {
            return Verdict::Holds;
        }
        let Source::Pool(pool) = pool else {
            return Verdict::Fails;
        };
        let Some(position) = self.analysis.parameter_position(pool) else {
            return Verdict::Fails;
        };
        let passed = match source {
            Source::Pool(local) => self.analysis.parameter_position(local).map(Passed::Pool),
            Source::Within(local) => self.analysis.parameter_position(local).map(Passed::Within),
            Source::Itself | Source::Temporary => None,
        };
        match passed {
            Some(passed) => Verdict::Requires(passed, position),
            None => Verdict::Fails,
        }
    }

    /// L0701: checks that `local`, given handles from `sources` at `span`, never keeps one after its pool is freed.
    fn kept(&mut self, local: LocalId, sources: &Sources, span: Span) {
        let holder = &self.function().locals[local];
        for &source in sources {
            let Source::Pool(pool) = source else {
                continue;
            };
            if pool == local || self.analysis.parameter_position(pool).is_some() {
                continue;
            }
            let pool_name = self.function().locals[pool].name;
            let diagnostic = if holder.by_reference {
                errors::handles_escape_call(holder.name, pool_name, self.function().name, span)
            } else {
                let pool_block = self.blocks.declared_in.get(&pool).copied().unwrap_or(0);
                let holder_block = self.blocks.declared_in.get(&local).copied().unwrap_or(0);
                if self.blocks.within(holder_block, pool_block) {
                    continue;
                }
                let declared = self.function().locals[pool].span;
                errors::handles_outlive_pool(holder.name, pool_name, span, declared)
            };
            self.report(diagnostic);
            return;
        }
    }

    /// L0701: checks that a returned value holds no handle into a pool the function frees as it returns.
    fn returns(&mut self, value: &Expr) {
        for source in self.analysis.sources_of(value) {
            if self.analysis.as_passed(source).is_some() {
                continue;
            }
            let diagnostic = errors::returned_handles_of_local(
                &self.pool_name(source),
                self.function().name,
                value.span,
            );
            self.report(diagnostic);
            return;
        }
    }

    /// The name an error gives `pool`, a pool a handle is used with, or one a returned handle comes from.
    fn pool_name(&self, pool: Source) -> String {
        match pool {
            Source::Pool(local) => self.function().locals[local].name.to_owned(),
            Source::Within(_) | Source::Itself | Source::Temporary => {
                "a pool with no binding".to_owned()
            }
        }
    }

    fn origin(&self, source: Source) -> Origin<'src> {
        match source {
            Source::Pool(local) => Origin::Pool(self.analysis.function.locals[local].name),
            Source::Within(local) => Origin::Within(self.analysis.function.locals[local].name),
            Source::Itself | Source::Temporary => Origin::Temporary,
        }
    }

    fn report(&mut self, diagnostic: Diagnostic) {
        if self.report {
            self.diagnostics.push(diagnostic);
        }
    }
}

/// Where a call passes the handles `passed` names, or `None` for those in the pool it returns.
fn argument_span(call: &Call, passed: Passed) -> Option<Span> {
    let (Passed::Pool(position) | Passed::Within(position)) = passed else {
        return None;
    };
    match call.arguments.get(position)? {
        Argument::Reference { span, .. } => Some(*span),
        Argument::Value(value) => Some(value.span),
    }
}
