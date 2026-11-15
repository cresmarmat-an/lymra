//! The wording of the errors about pools and handles: misused `insert`, `remove` and `pool[handle]`, and L0706.

use super::described;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::Type;
use crate::source::Span;

/// What changes a pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::semantic) enum PoolChange {
    Insert,
    Remove,
}

impl PoolChange {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Remove => "remove",
        }
    }
}

/// `handle` written without exactly one type in brackets.
pub(in crate::semantic) fn malformed_handle_type(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0409,
        "a handle type names the type of the value it names",
        Label::new(span, "expected `handle[value type]`"),
        Help::text("write the type of the values in its pool, as in `handle[Creature]`"),
    )
}

/// `insert` or `remove` given the wrong number of arguments.
pub(in crate::semantic) fn pool_argument_count(
    change: PoolChange,
    found: usize,
    call: Span,
) -> Diagnostic {
    let given = if found == 1 { "was" } else { "were" };
    let (label, help) = match change {
        PoolChange::Insert => (
            "`insert` stores one value in one pool",
            "write `insert(variable creatures, value)`",
        ),
        PoolChange::Remove => (
            "`remove` takes one value out of one pool",
            "write `remove(variable creatures, handle)`",
        ),
    };
    Diagnostic::new(
        ErrorCode::L0501,
        format!(
            "`{}` takes 2 arguments, but {found} {given} given",
            change.name()
        ),
        Label::new(call, label),
        Help::text(help),
    )
}

/// A pool given to `insert` or `remove` without `variable`. `source` is the argument as written.
pub(in crate::semantic) fn pool_needs_variable(
    change: PoolChange,
    span: Span,
    source: &str,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0504,
        format!(
            "`{}` changes the pool it is given, so the pool needs `variable`",
            change.name()
        ),
        Label::new(span, "passed without `variable`"),
        Help::replace(
            "mark the pool `variable`",
            span,
            format!("variable {source}"),
        ),
    )
}

/// The value or the handle given to `insert` or `remove` with `variable`, which the call only reads.
pub(in crate::semantic) fn pool_argument_marked(change: PoolChange, mark: Span) -> Diagnostic {
    let what = match change {
        PoolChange::Insert => "the value it stores",
        PoolChange::Remove => "the handle it is given",
    };
    Diagnostic::new(
        ErrorCode::L0504,
        format!(
            "`{}` only reads {what}, so it takes no `variable`",
            change.name()
        ),
        Label::new(mark, "not needed here"),
        Help::replace("remove `variable`", mark, String::new()),
    )
}

/// `insert` or `remove` given something other than a pool.
pub(in crate::semantic) fn not_a_pool(change: PoolChange, found: &Type, span: Span) -> Diagnostic {
    let help = match (change, found) {
        (PoolChange::Insert, Type::List(_)) => {
            "add to a list with `push`; `insert` stores a value in a pool and gives back its handle"
        }
        _ => "pass a `pool` binding, marked `variable`, as in `insert(variable creatures, value)`",
    };
    Diagnostic::new(
        ErrorCode::L0402,
        format!(
            "`{}` works on a pool, but this is {}",
            change.name(),
            described(found)
        ),
        Label::new(span, "not a pool"),
        Help::text(help),
    )
}

/// A pool of type `pool` reached through something other than a handle to its values.
pub(in crate::semantic) fn slot_not_handle(pool: &Type, found: &Type, span: Span) -> Diagnostic {
    let help = match found {
        Type::Handle(_) => "use a handle that `insert` gave for a pool of this type",
        Type::Integer(_) => {
            "a pool keeps its values in no order, so it has no positions; use the handle `insert` gave when the value went in"
        }
        _ => "use the handle `insert` gave when the value went in",
    };
    let expected = pool.as_pool().map_or_else(
        || "handle".to_owned(),
        |pool| Type::handle(pool.element.clone()).to_string(),
    );
    Diagnostic::new(
        ErrorCode::L0402,
        format!(
            "{} takes a `{expected}`, but this is {}",
            described(pool),
            described(found)
        ),
        Label::new(span, "not a handle to its values"),
        Help::text(help),
    )
}

/// L0706: a pool of values whose size is not fixed.
pub(in crate::semantic) fn pool_element_not_fixed(element: &Type, span: Span) -> Diagnostic {
    let (reason, help) = if element.is_growable() {
        (
            "grows while it lives",
            "keep it in a binding of its own, and give the pool's values what they need to find it",
        )
    } else {
        (
            "holds text, which can have any length",
            "keep the text in a list, and store its position in the pool's values",
        )
    };
    Diagnostic::new(
        ErrorCode::L0706,
        format!(
            "a pool holds values of one fixed size, but {} {reason}",
            described(element)
        ),
        Label::new(span, "not a value of one fixed size"),
        Help::text(help),
    )
}

/// Where a handle can come from, as an error about it says.
pub(in crate::semantic) enum Origin<'a> {
    /// The pool a binding or a parameter holds.
    Pool(&'a str),
    /// The pools the handles a parameter brings from the caller come from.
    Within(&'a str),
    /// A pool computed without a binding.
    Temporary,
}

impl Origin<'_> {
    fn describe(&self) -> String {
        match self {
            Self::Pool(pool) => format!("`{pool}`"),
            Self::Within(parameter) => {
                format!("whichever pool the caller's `{parameter}` holds handles into")
            }
            Self::Temporary => "a pool with no binding".to_owned(),
        }
    }
}

/// L0705: a handle, at `span`, used with `pool`, though it can come from `origin`.
pub(in crate::semantic) fn handle_from_another_pool(
    span: Span,
    pool: &str,
    origin: &Origin<'_>,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0705,
        format!(
            "this handle can come from {}, so it cannot be used with `{pool}`",
            origin.describe()
        ),
        Label::new(span, format!("used with `{pool}` here")),
        Help::text(format!(
            "use each handle with the pool whose `insert` gave it; one used with `{pool}` has to come from `{pool}`"
        )),
    )
}

/// L0705 at a call: `function` uses the handles passed at `span` with `pool`, though they can come from `origin`.
pub(in crate::semantic) fn call_mixes_pools(
    span: Span,
    function: &str,
    pool: &str,
    origin: &Origin<'_>,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0705,
        format!(
            "`{function}` uses the handles it gets here with `{pool}`, but they can come from {}",
            origin.describe()
        ),
        Label::new(span, format!("`{function}` uses these with `{pool}`")),
        Help::text(format!(
            "pass handles that come from `{pool}`, or pass the pool they come from"
        )),
    )
}

/// L0701: `holder`, which gets handles into `pool` at `span`, declared in a block that ends after the pool's.
pub(in crate::semantic) fn handles_outlive_pool(
    holder: &str,
    pool: &str,
    span: Span,
    declared: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{holder}` would keep handles into `{pool}` after `{pool}` is freed"),
        Label::new(span, format!("`{holder}` gets a handle into `{pool}` here")),
        Help::text(format!(
            "use `{holder}` only inside the block of `{pool}`, or declare `{pool}` where `{holder}` is"
        )),
    )
    .with_secondary(Label::new(
        declared,
        format!("`{pool}` is freed at the end of its block"),
    ))
}

/// L0701: a function that returns handles into `pool`, which it frees as it returns.
pub(in crate::semantic) fn returned_handles_of_local(
    pool: &str,
    function: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{function}` returns a handle into `{pool}`, which is freed when it returns"),
        Label::new(span, format!("a handle into `{pool}`")),
        Help::text(format!(
            "take the pool as a `variable` parameter instead of declaring `{pool}`, so the caller keeps it"
        )),
    )
}

/// L0701: a `variable` parameter given handles into `pool`, a binding of `function`, which the caller would keep.
pub(in crate::semantic) fn handles_escape_call(
    parameter: &str,
    pool: &str,
    function: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!(
            "`{parameter}` would keep handles into `{pool}`, which is freed when `{function}` returns"
        ),
        Label::new(
            span,
            format!("the caller keeps `{parameter}` after the call"),
        ),
        Help::text(format!(
            "take the pool as a `variable` parameter too, so the caller keeps the pool along with `{parameter}`"
        )),
    )
}
