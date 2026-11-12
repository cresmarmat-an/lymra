/*!
 * `#!prove bounds`: every index in the file has to be proven inside its
 * array while compiling. An index the analysis cannot prove is error
 * `L0104`, which says what the analysis knew about it there.
 */

use super::interval::Interval;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::{Expr, IntegerType};
use crate::source::{SourceFile, Span};

/// An index the analysis could not prove inside its array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnprovenIndex {
    pub span: Span,
    /// The values the index can have there, as far as the analysis knows.
    known: Interval,
    /// The values of the index's type, which are all it can have when nothing is known.
    whole: Interval,
    /// The name of the index's type.
    ty: String,
    pub length: u64,
}

impl UnprovenIndex {
    pub(super) fn new(index: &Expr, known: Option<Interval>, length: u64) -> Self {
        // Every index is an integer; the widest one stands in should that ever change.
        let whole = Interval::of_type(index.ty.as_integer().unwrap_or(IntegerType::Integer64));
        Self {
            span: index.span,
            known: known.unwrap_or(whole),
            whole,
            ty: index.ty.to_string(),
            length,
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
        let last = self.length.saturating_sub(1);
        let index = source.slice(self.span);
        let too_low = self.known.low < 0;
        let too_high = self.known.high > i128::from(last);
        let condition = match (too_low, too_high) {
            (true, false) => format!("{index} >= 0"),
            (false, true) => format!("{index} < {}", self.length),
            (true, true) | (false, false) => {
                format!("{index} >= 0 and {index} < {}", self.length)
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
}
