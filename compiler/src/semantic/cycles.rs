/*!
 * Declarations whose definitions need other declarations: a type needs the
 * types of its fields and the constants that give their array lengths, and
 * a top-level constant needs whatever its value names. Each is worked out
 * when something first needs it, so declarations can come in any order.
 * One that leads back to itself has no definition; the error shows the way
 * around.
 */

use super::errors::{self, Declared, Dependency};
use super::{Checker, Definition};
use crate::diagnostics::Diagnostic;
use crate::source::Span;

impl<'src> Checker<'src, '_> {
    /// Runs `work`, which defines `definition`, named at `reference`, while keeping it on the path.
    pub(super) fn defining<T>(
        &mut self,
        definition: Definition,
        reference: Span,
        work: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.resolving.push((definition, reference));
        let result = work(self);
        self.resolving.pop();
        result
    }

    /**
     * The error for `definition`, which is being worked out, named again
     * at `closing`. The way around runs through the definitions started
     * since, each named by the one before.
     */
    pub(super) fn cycle(&self, definition: Definition, closing: Span) -> Diagnostic {
        let start = self
            .resolving
            .iter()
            .position(|&(started, _)| started == definition)
            .unwrap_or(0);
        let path = &self.resolving[start..];
        let earlier: Vec<Dependency<'src>> = path
            .iter()
            .zip(path.iter().skip(1))
            .map(|(&(holder, _), &(held, span))| Dependency {
                holder: self.declared(holder),
                held: self.declared(held),
                span,
            })
            .collect();
        let last = path.last().map_or(definition, |&(last, _)| last);
        let closing = Dependency {
            holder: self.declared(last),
            held: self.declared(definition),
            span: closing,
        };
        errors::cycle(self.declaration(definition), &earlier, &closing)
    }

    fn declared(&self, definition: Definition) -> Declared<'src> {
        match definition {
            Definition::Type(id) => Declared::Type(
                self.types
                    .get(id)
                    .map_or("", |entry| entry.declaration.name.text),
            ),
            Definition::Constant(id) => Declared::Constant(
                self.constants
                    .get(id)
                    .map_or("", |entry| entry.binding.name.text),
            ),
        }
    }

    /// Where `definition` is declared: the name after `type` or `constant`.
    fn declaration(&self, definition: Definition) -> Span {
        let name = match definition {
            Definition::Type(id) => self.types.get(id).map(|entry| entry.declaration.name),
            Definition::Constant(id) => self.constants.get(id).map(|entry| entry.binding.name),
        };
        name.map_or(Span::empty(0), |name| name.span)
    }
}
