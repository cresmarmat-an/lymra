//! The wording of the errors about what views show: L0701, L0702 and L0703.

use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::ir::Sequence;
use crate::source::Span;

/// How a value frees what a view or a `for` loop may still read, where L0702 or L0703 points.
#[derive(Clone, Copy, Debug)]
pub(in crate::semantic) enum Freeing<'a> {
    /// Assigned a whole new value; the span is the target as written.
    Assigned(Span),
    /// Given new text in a part, which rewrites all of its text; the span is the target as written.
    NewTextInPart(Span),
    /// Passed with `variable` to the function named, which can replace it; the span is the argument as written.
    PassedToReplace(Span, &'a str),
    /// Popped; the span is the `pop` call.
    Popped(Span),
    /// Passed with `variable` to the function named, which can pop it; the span is the argument as written.
    PassedToPop(Span, &'a str),
}

impl Freeing<'_> {
    fn code(self) -> ErrorCode {
        match self {
            Self::Assigned(_) | Self::NewTextInPart(_) | Self::PassedToReplace(..) => {
                ErrorCode::L0703
            }
            Self::Popped(_) | Self::PassedToPop(..) => ErrorCode::L0702,
        }
    }

    fn span(self) -> Span {
        match self {
            Self::Assigned(span)
            | Self::NewTextInPart(span)
            | Self::PassedToReplace(span, _)
            | Self::Popped(span)
            | Self::PassedToPop(span, _) => span,
        }
    }

    /// What the error says the value cannot be: "replaced", "popped", or passed on.
    fn verb(self) -> &'static str {
        match self {
            Self::Assigned(_) => "replaced",
            Self::NewTextInPart(_) => "given new text",
            Self::Popped(_) => "popped",
            Self::PassedToReplace(..) | Self::PassedToPop(..) => "passed with `variable`",
        }
    }
}

/// What keeps the elements that a view or a `for` loop reads, which picks the words of L0702 and L0703.
#[derive(Clone, Copy, Debug)]
pub(in crate::semantic) enum Owner {
    /// A list or a stack.
    Sequence(Sequence),
    /// Text, or a value that holds text, whose bytes a view shows.
    Text,
}

impl Owner {
    /// What a `for` loop goes over, said of the owner.
    const fn looped_over(self) -> &'static str {
        match self {
            Self::Sequence(_) => "it",
            Self::Text => "its text",
        }
    }
}

/// `value`, kept as `owner` says, that frees elements as `how` says while a `for` loop goes over it, or over a view of it, named at `iterable`.
pub(in crate::semantic) fn freed_while_iterated(
    value: &str,
    owner: Owner,
    how: Freeing<'_>,
    iterable: Span,
) -> Diagnostic {
    let new_text_after = format!(
        "build the new text in another binding inside the loop, and give it to `{value}` after the loop ends"
    );
    let (label, help) = match (owner, how) {
        (Owner::Sequence(kind), Freeing::Assigned(_) | Freeing::NewTextInPart(_)) => (
            format!("this replaces the whole {}", kind.name()),
            "change its elements or `push` to it inside the loop, and replace it after the loop ends"
                .to_owned(),
        ),
        (Owner::Sequence(kind), Freeing::PassedToReplace(_, function)) => (
            format!("`{function}` can replace the whole {}", kind.name()),
            format!(
                "call `{function}` after the loop; `push` and changing elements are fine inside it"
            ),
        ),
        (Owner::Text, Freeing::Assigned(_)) => {
            ("this frees the text the loop goes over".to_owned(), new_text_after)
        }
        (Owner::Text, Freeing::NewTextInPart(_)) => (
            format!("this rewrites all of `{value}`'s text, which the loop goes over"),
            new_text_after,
        ),
        (Owner::Text, Freeing::PassedToReplace(_, function)) => (
            format!("`{function}` can give `{value}` new text"),
            format!("call `{function}` after the loop"),
        ),
        (Owner::Sequence(_) | Owner::Text, Freeing::Popped(_)) => (
            format!("this takes the top element off `{value}`"),
            format!(
                "to take each element off in turn, loop with `while length({value}) > 0` and `pop` inside it"
            ),
        ),
        (Owner::Sequence(_) | Owner::Text, Freeing::PassedToPop(_, function)) => (
            format!("`{function}` can take elements off `{value}`"),
            format!(
                "call `{function}` after the loop, or loop with `while length({value}) > 0` instead of `for`"
            ),
        ),
    };
    Diagnostic::new(
        how.code(),
        format!(
            "`{value}` cannot be {} while a `for` loop goes over {}",
            how.verb(),
            owner.looped_over()
        ),
        Label::new(how.span(), label),
        Help::text(help),
    )
    .with_secondary(Label::new(iterable, "the loop goes over it here"))
}

/// `value`, kept as `owner` says, that frees elements as `how` says while `view`, declared at `declared`, still shows it.
pub(in crate::semantic) fn freed_while_viewed(
    value: &str,
    owner: Owner,
    view: &str,
    how: Freeing<'_>,
    declared: Span,
) -> Diagnostic {
    let label = match (owner, how) {
        (Owner::Sequence(_), Freeing::Assigned(_) | Freeing::NewTextInPart(_)) => {
            format!("this frees the elements `{view}` shows")
        }
        (Owner::Sequence(kind), Freeing::PassedToReplace(_, function)) => {
            format!("`{function}` can replace the whole {}", kind.name())
        }
        (Owner::Text, Freeing::Assigned(_)) => format!("this frees the text `{view}` shows"),
        (Owner::Text, Freeing::NewTextInPart(_)) => {
            format!("this rewrites all of `{value}`'s text, which `{view}` shows")
        }
        (Owner::Text, Freeing::PassedToReplace(_, function)) => {
            format!("`{function}` can give `{value}` new text")
        }
        (Owner::Sequence(_) | Owner::Text, Freeing::Popped(_)) => {
            format!("this frees the top element, which `{view}` can show")
        }
        (Owner::Sequence(_) | Owner::Text, Freeing::PassedToPop(_, function)) => {
            format!("`{function}` can take elements off `{value}`")
        }
    };
    let keep = match owner {
        Owner::Sequence(_) => format!("copy what `{view}` shows into a list of its own"),
        Owner::Text => {
            format!("copy the text first with `constant kept = {value}`, and show `kept`")
        }
    };
    Diagnostic::new(
        how.code(),
        format!(
            "`{value}` cannot be {} while `{view}`, a view of it, is still used",
            how.verb()
        ),
        Label::new(how.span(), label),
        Help::text(format!(
            "finish with `{view}` before `{value}` changes, or {keep}"
        )),
    )
    .with_secondary(Label::new(declared, format!("`{view}` shows `{value}`")))
}

/// What still uses the handles into a pool where it is replaced.
#[derive(Clone, Copy, Debug)]
pub(in crate::semantic) enum Holder<'a> {
    /// A `for` loop going over handles into it, named at the span.
    Loop(Span),
    /// A binding that holds handles into it, declared at the span.
    Binding(&'a str, Span),
}

/// L0703: `pool` replaced, as `how` says, while `holder` still uses handles into it.
pub(in crate::semantic) fn pool_replaced_while_handled(
    pool: &str,
    how: Freeing<'_>,
    holder: Holder<'_>,
) -> Diagnostic {
    let user = match holder {
        Holder::Loop(_) => "a `for` loop goes over handles into it".to_owned(),
        Holder::Binding(name, _) => format!("`{name}`, which holds handles into it, is still used"),
    };
    let label = match how {
        Freeing::Assigned(_) | Freeing::NewTextInPart(_) | Freeing::Popped(_) => {
            "this frees every value in it, and its handles would name the slots of the new pool"
                .to_owned()
        }
        Freeing::PassedToReplace(_, function) | Freeing::PassedToPop(_, function) => {
            format!("`{function}` can replace the whole pool")
        }
    };
    let secondary = match holder {
        Holder::Loop(span) => Label::new(span, "the loop goes over these handles"),
        Holder::Binding(name, declared) => {
            Label::new(declared, format!("`{name}` holds handles into `{pool}`"))
        }
    };
    Diagnostic::new(
        how.code(),
        format!("`{pool}` cannot be {} while {user}", how.verb()),
        Label::new(how.span(), label),
        Help::text(format!(
            "replace `{pool}` after the last use of its handles, or take out the values you are done with using `remove`"
        )),
    )
    .with_secondary(secondary)
}

/// A view binding given a view of a value its statement frees.
pub(in crate::semantic) fn view_of_temporary(view: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{view}` would show a value that is freed at the end of this statement"),
        Label::new(
            span,
            "this value has no binding, so it lasts only as long as its statement",
        ),
        Help::text(format!(
            "keep the value in a binding first, then give `{view}` a view of that binding"
        )),
    )
}

/// A view binding given a view of `root`, declared at `declared` in a block that ends before the view's.
pub(in crate::semantic) fn view_outlives_root(
    view: &str,
    root: &str,
    span: Span,
    declared: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{view}` would outlive `{root}`, which it shows"),
        Label::new(span, format!("`{view}` shows `{root}` from here")),
        Help::text(format!(
            "use `{view}` only inside the block of `{root}`, or keep what it shows in a binding declared where `{view}` is"
        )),
    )
    .with_secondary(Label::new(
        declared,
        format!("`{root}` is freed at the end of its block, before `{view}` is"),
    ))
}

/// A `variable` view parameter given a view of `root`, a binding of `function`, which the caller would keep.
pub(in crate::semantic) fn view_escapes_call(
    view: &str,
    root: &str,
    function: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{view}` would show `{root}`, which is freed when `{function}` returns"),
        Label::new(span, format!("the caller keeps `{view}` after the call")),
        Help::text(format!(
            "give `{view}` a part of what it showed, or a view of another parameter"
        )),
    )
}

/// A function that returns a view of a value its `return` frees.
pub(in crate::semantic) fn returned_view_of_temporary(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        "this returns a view of a value that is freed at the end of the `return`",
        Label::new(
            span,
            "this value has no binding, so it lasts only as long as its statement",
        ),
        Help::text("return the value itself, as a list or an array, instead of a view of it"),
    )
}

/// A function that returns a view of `root`, one of its own bindings.
pub(in crate::semantic) fn returned_view_of_local(
    root: &str,
    function: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0701,
        format!("`{function}` returns a view of `{root}`, which is freed when it returns"),
        Label::new(span, format!("a view of `{root}`")),
        Help::text(format!(
            "return `{root}` itself, or a view of one of the parameters, which the caller keeps"
        )),
    )
}
