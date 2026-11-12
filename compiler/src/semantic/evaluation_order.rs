/*!
 * One expression may not change a binding through a `variable` argument and
 * also use that binding anywhere else. The parts of an expression run from
 * left to right, but which part changes what another reads is easy to
 * misread. The rule also lets code generation keep only the order of the
 * parts with effects, since no part can change what another part reads.
 */

use crate::ast::visit::{self, Visitor};
use crate::ast::{self, ArgumentKind};
use crate::source::Span;

/// A binding that one expression passes with `variable` and also uses somewhere else.
pub(super) struct Conflict<'src> {
    pub name: &'src str,
    /// The function that receives the binding, when the call names one.
    pub function: Option<&'src str>,
    /// The `variable name` argument.
    pub changed: Span,
    /// Another place the expression uses the binding.
    pub used: Span,
}

/// A binding passed with `variable`.
struct Passed<'src> {
    name: &'src str,
    function: Option<&'src str>,
    span: Span,
}

/// Every binding an expression reads, and every binding it passes with `variable`.
#[derive(Default)]
struct Uses<'src> {
    reads: Vec<(&'src str, Span)>,
    passed: Vec<Passed<'src>>,
}

impl<'src> Visitor<'src> for Uses<'src> {
    fn visit_expr(&mut self, expr: &ast::Expr<'src>) {
        let ast::ExprKind::Call(call) = &expr.kind else {
            if let ast::ExprKind::Name(name) = &expr.kind {
                self.reads.push((*name, expr.span));
            }
            visit::walk_expr(self, expr);
            return;
        };
        // A function's name is not a binding, so only a callee that is not a plain name can use one.
        let function = match &call.callee.kind {
            ast::ExprKind::Name(name) => Some(*name),
            _ => {
                self.visit_expr(&call.callee);
                None
            }
        };
        for argument in &call.arguments {
            match &argument.kind {
                ArgumentKind::Mutable(place) => match &place.kind {
                    ast::ExprKind::Name(name) => self.passed.push(Passed {
                        name,
                        function,
                        span: argument.span,
                    }),
                    _ => self.visit_expr(place),
                },
                ArgumentKind::Positional(value) | ArgumentKind::Named { value, .. } => {
                    self.visit_expr(value);
                }
            }
        }
    }
}

/// The first binding, in source order, that `expr` passes with `variable` and also uses elsewhere.
pub(super) fn find_conflict<'src>(expr: &ast::Expr<'src>) -> Option<Conflict<'src>> {
    let mut uses = Uses::default();
    uses.visit_expr(expr);
    uses.passed.iter().find_map(|passed| {
        let reads = uses
            .reads
            .iter()
            .filter(|(name, _)| *name == passed.name)
            .map(|&(_, span)| span);
        let passed_again = uses
            .passed
            .iter()
            .filter(|other| other.name == passed.name && other.span != passed.span)
            .map(|other| other.span);
        let used = reads.chain(passed_again).min_by_key(|span| span.start)?;
        Some(Conflict {
            name: passed.name,
            function: passed.function,
            changed: passed.span,
            used,
        })
    })
}
