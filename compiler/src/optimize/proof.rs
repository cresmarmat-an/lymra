/*!
 * `#!prove bounds`: every index in the file has to be proven inside its
 * array while compiling. An index the analysis cannot prove is error
 * `L0104`, which says what the analysis knew about it there.
 */

use super::interval::Interval;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::{Expr, Extent, IntegerType};
use crate::source::{SourceFile, Span};

/// What an index has to stay below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bound {
    /// An array's length.
    Fixed(u64),
    /// The length of a list, which only the running program knows. The span names the list.
    List(Span),
}

impl Bound {
    /// The bound of an index with `length`, into the value written at `indexed`.
    pub fn of(length: Extent, indexed: Span) -> Bound {
        match length {
            Extent::Fixed(length) => Bound::Fixed(length),
            Extent::Dynamic => Bound::List(indexed),
        }
    }
}

/// An index the analysis could not prove inside its array or list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnprovenIndex {
    pub span: Span,
    /// The values the index can have there, as far as the analysis knows.
    known: Interval,
    /// The values of the index's type, which are all it can have when nothing is known.
    whole: Interval,
    /// The name of the index's type.
    ty: String,
    bound: Bound,
}

impl UnprovenIndex {
    pub(super) fn new(index: &Expr, known: Option<Interval>, bound: Bound) -> Self {
        // Every index is an integer; the widest one stands in should that ever change.
        let whole = Interval::of_type(index.ty.as_integer().unwrap_or(IntegerType::Integer64));
        Self {
            span: index.span,
            known: known.unwrap_or(whole),
            whole,
            ty: index.ty.to_string(),
            bound,
        }
    }

    /// What is known about the index, in words.
    fn description(&self) -> String {
        let Interval { low, high } = self.known;
        match (low == self.whole.low, high == self.whole.high) {
            (true, true) => format!("this can be any `{}` here", self.ty),
            (true, false) => format!("this can be at most {high}"),
            (false, true) => format!("this can be at least {low}"),
            (false, false) => format!("this can be from {low} to {high}"),
        }
    }

    /// The error for this index. `source` holds the index as written, for the suggested `assert`.
    pub fn diagnostic(&self, source: &SourceFile) -> Diagnostic {
        let length = match self.bound {
            Bound::Fixed(length) => length,
            Bound::List(list) => return self.list_diagnostic(source, list),
        };
        let last = length.saturating_sub(1);
        let index = source.slice(self.span);
        let too_low = self.known.low < 0;
        let too_high = self.known.high > i128::from(last);
        let condition = match (too_low, too_high) {
            (true, false) => format!("{index} >= 0"),
            (false, true) => format!("{index} < {length}"),
            (true, true) | (false, false) => {
                format!("{index} >= 0 and {index} < {length}")
            }
        };
        Diagnostic::new(
            ErrorCode::L0104,
            "this index is not proven to stay inside its array",
            Label::new(
                self.span,
                format!(
                    "{}; the array's indexes are 0 to {last}",
                    self.description()
                ),
            ),
            Help::text(format!(
                "`#!prove bounds` needs every index proven: check it before this line, with an `if` \
                 or with `assert {condition}`, or loop over a range that stays inside the array"
            )),
        )
    }

    /// The error for an index into a list, written at `list`, whose length only the running program knows.
    fn list_diagnostic(&self, source: &SourceFile, list: Span) -> Diagnostic {
        let index = source.slice(self.span);
        let list = source
            .slice(list)
            .trim_end()
            .trim_end_matches('[')
            .trim_end();
        let condition = if self.known.low < 0 {
            format!("{index} >= 0 and {index} < length({list})")
        } else {
            format!("{index} < length({list})")
        };
        Diagnostic::new(
            ErrorCode::L0104,
            "this index is not proven to stay inside its list",
            Label::new(
                self.span,
                format!(
                    "{}; a list's length is only known when the program runs",
                    self.description()
                ),
            ),
            Help::text(format!(
                "`#!prove bounds` needs every index proven: check it before this line, with an `if` \
                 or with `assert {condition}`, or loop over `range(0, length({list}))`"
            )),
        )
    }
}
