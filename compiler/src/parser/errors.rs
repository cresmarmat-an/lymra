//! The wording of every error the parser reports.

use super::{MAX_NESTING, MAX_OPERATIONS};
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::source::Span;

const KNOWN_DIRECTIVES: &str = "`#!edition 0.1`, `#!optimize speed`, `#!optimize size`, `#!optimize debug` and `#!prove bounds`";

pub(super) fn unexpected_token(expected: &str, found: &str, span: Span, help: Help) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        format!("expected {expected}, found {found}"),
        Label::new(span, format!("expected {expected}")),
        help,
    )
}

pub(super) fn keyword_as_name(keyword: &str, expected: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        format!("`{keyword}` is a keyword, so it cannot be used as a name"),
        Label::new(span, format!("expected {expected}")),
        Help::text("choose a different name"),
    )
}

/// Reports a list or group whose closing bracket did not come where it had to.
pub(super) fn unclosed_bracket(
    expected: &str,
    closer: &str,
    found: &str,
    found_span: Span,
    opener: Span,
    at_line_end: bool,
) -> Diagnostic {
    let help = if at_line_end {
        Help::insert(format!("add `{closer}`"), found_span.start, closer)
    } else {
        Help::text(format!("close the bracket with `{closer}` before this"))
    };
    Diagnostic::new(
        ErrorCode::L0301,
        format!("expected {expected}, found {found}"),
        Label::new(found_span, format!("expected {expected}")),
        help,
    )
    .with_secondary(Label::new(opener, "this bracket is still open"))
}

pub(super) fn trailing_comma(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        "a list cannot end with a comma",
        Label::new(span, "nothing follows this comma"),
        Help::remove("remove the comma", span),
    )
}

pub(super) fn empty_parentheses(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        "empty parentheses",
        Label::new(span, "no expression between `(` and `)`"),
        Help::text("write a value inside the parentheses, or remove them"),
    )
}

pub(super) fn empty_variant_fields(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        "a variant without fields is written without parentheses",
        Label::new(span, "nothing between `(` and `)`"),
        Help::remove("remove the parentheses", span),
    )
}

pub(super) fn empty_type_arguments(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        "empty type argument list",
        Label::new(span, "no type between `[` and `]`"),
        Help::text("write what the type holds, as in `list[integer32]`"),
    )
}

pub(super) fn empty_interpolation(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0113,
        "empty interpolation",
        Label::new(span, "nothing between `{` and `}`"),
        Help::replace(
            "put an expression between the braces, or double them for literal braces",
            span,
            "{{}}",
        ),
    )
}

pub(super) fn nesting_too_deep(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0310,
        "this code is nested too deeply",
        Label::new(span, format!("more than {MAX_NESTING} levels of nesting")),
        Help::text("move the inner part into its own function or constant"),
    )
}

pub(super) fn expression_too_long(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0310,
        "this expression is too long",
        Label::new(
            span,
            format!(
                "more than {MAX_OPERATIONS} operators, calls, indexes and fields in one expression"
            ),
        ),
        Help::text("split it up: compute parts of it into constants, then combine them"),
    )
}

pub(super) fn missing_block(header: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0303,
        "expected an indented block",
        Label::new(
            header,
            "the lines that belong to this have to follow it, indented one level",
        ),
        Help::text("indent the body one level deeper than this line"),
    )
}

pub(super) fn unexpected_indentation(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0304,
        "unexpected indentation",
        Label::new(span, "this line is indented, but no block starts above it"),
        Help::text("indent it like the line above, or start a block above it"),
    )
}

pub(super) fn statement_at_top_level(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0302,
        "this statement is outside a function",
        Label::new(span, "only declarations can be at the top level of a file"),
        Help::text("move it into a function; a program starts in `function main()`"),
    )
}

pub(super) fn declaration_in_function(keyword: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0302,
        format!("a `{keyword}` declaration cannot be inside a function"),
        Label::new(span, "declared inside a function body"),
        Help::text("move it to the top level of the file"),
    )
}

pub(super) fn late_directive(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0101,
        "a directive has to come before the first declaration",
        Label::new(span, "this directive comes too late"),
        Help::text("move this line to the top of the file"),
    )
}

/// `name_span` covers the word after `#!`, which a close match replaces.
pub(super) fn unknown_directive(name: &str, name_span: Span, closest: Option<&str>) -> Diagnostic {
    let span = name_span;
    let help = match closest {
        Some(closest) => Help::replace(format!("did you mean `#!{closest}`?"), span, closest),
        None => Help::text(format!("the directives are {KNOWN_DIRECTIVES}")),
    };
    Diagnostic::new(
        ErrorCode::L0105,
        format!("unknown directive `#!{name}`"),
        Label::new(span, "not a directive Lymra knows"),
        help,
    )
}

/// Reports `#!nocheck` and `#!nonstrict`, which the spec rejects by name.
pub(super) fn safety_switch_directive(name: &str, line: Span) -> Diagnostic {
    let help = if name == "nocheck" {
        Help::replace(
            "to drop bounds checks, make the compiler prove them unnecessary",
            line,
            "#!prove bounds",
        )
    } else {
        Help::remove("remove this line", line)
    };
    Diagnostic::new(
        ErrorCode::L0105,
        format!("`#!{name}` does not exist: no directive turns safety checks off"),
        Label::new(line, "safety checks cannot be disabled"),
        help,
    )
}

pub(super) fn strict_directive(line: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0105,
        "`#!strict` does not exist: Lymra is always strict",
        Label::new(line, "there is no relaxed mode to opt out of"),
        Help::remove("remove this line", line),
    )
}

pub(super) fn directive_misuse(message: String, label: Label, help: Help) -> Diagnostic {
    Diagnostic::new(ErrorCode::L0106, message, label, help)
}

pub(super) fn duplicate_directive(name: &str, second: Span, first: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0106,
        format!("`#!{name}` is given twice"),
        Label::new(second, "second use"),
        Help::remove("remove one of them", second),
    )
    .with_secondary(Label::new(first, "first use"))
}

pub(super) fn declaration_without_keyword(declaration: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0203,
        "this declaration needs `variable` or `constant` in front",
        Label::new(declaration, "a declaration starts with a keyword"),
        Help::insert(
            "write `constant` for a value that never changes, or `variable` for one that does",
            declaration.start,
            "constant ",
        ),
    )
}

pub(super) fn missing_initializer(name: &str, declaration: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0204,
        format!("`{name}` is declared without a value"),
        Label::new(declaration, "a declaration has to give its binding a value"),
        Help::text(format!(
            "write `= <value>` after `{name}`; Lymra has no uninitialized bindings"
        )),
    )
}

pub(super) fn variable_at_top_level(keyword: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0205,
        "a `variable` cannot be declared outside a function",
        Label::new(keyword, "changeable state at the top level of a file"),
        Help::replace(
            "make it a `constant` if it never changes, or declare it inside the function that uses it",
            keyword,
            "constant",
        ),
    )
}

pub(super) fn type_after_name(span: Span, name: &str, ty: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        "the type goes before the name in Lymra",
        Label::new(span, "written as `name: type` here"),
        Help::replace(
            "write the type first, without `:`",
            span,
            format!("{ty} {name}"),
        ),
    )
}

pub(super) fn missing_type(role: &str, name: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        format!("{role} `{name}` has no type"),
        Label::new(span, "a type has to come before this name"),
        Help::text(format!(
            "write the type first, for example `integer64 {name}`"
        )),
    )
}

/// Reports a type name from another language, such as `i64` for `integer64`.
pub(super) fn foreign_keyword(span: Span, found: &str, lymra: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        format!("`{found}` is not a Lymra keyword"),
        Label::new(span, format!("Lymra spells this `{lymra}`")),
        Help::replace(format!("write `{lymra}`"), span, lymra),
    )
}

pub(super) fn foreign_binding_keyword(span: Span, found: &str, lymra: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        format!("`{found}` is not a Lymra keyword"),
        Label::new(span, "a declaration starts with `variable` or `constant`"),
        Help::replace(
            "write `constant` for a value that never changes, or `variable` for one that does",
            span,
            lymra,
        ),
    )
}

pub(super) fn misspelled_keyword(span: Span, found: &str, keyword: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        format!("`{found}` is not a keyword"),
        Label::new(span, format!("did you mean `{keyword}`?")),
        Help::replace(format!("write `{keyword}`"), span, keyword),
    )
}

pub(super) fn foreign_return_type(span: Span, found: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        format!("`{found}` does not introduce a return type in Lymra"),
        Label::new(span, "Lymra spells this `returns`"),
        Help::replace("write `returns`", span, "returns"),
    )
}

/// `removal` covers the word and the space before it, so the fixed line has no stray gap.
pub(super) fn foreign_block_opener(span: Span, found: &str, removal: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        format!("`{found}` does not start a block in Lymra"),
        Label::new(span, "a block starts on the next line, indented one level"),
        Help::remove(format!("remove `{found}`"), removal),
    )
}

pub(super) fn semicolon(span: Span, ends_line: bool) -> Diagnostic {
    let help = if ends_line {
        Help::remove("remove `;`", span)
    } else {
        Help::text("put each statement on its own line, without `;`")
    };
    Diagnostic::new(
        ErrorCode::L0305,
        "`;` does not end a statement in Lymra",
        Label::new(span, "a statement ends at the end of its line"),
        help,
    )
}

pub(super) fn foreign_assignment_operator(
    operator: Span,
    found: &str,
    statement: Span,
    rewritten: String,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        format!("`{found}` is not a Lymra operator"),
        Label::new(operator, "assignment is always written with a plain `=`"),
        Help::replace("spell the operation out", statement, rewritten),
    )
}

pub(super) fn foreign_range(span: Span, rewritten: String) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        "`..` does not make a range in Lymra",
        Label::new(span, "a range is made with `range(start, end)`"),
        Help::replace("call `range`", span, rewritten),
    )
}

pub(super) fn foreign_use_from(span: Span, rewritten: String) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        "`use ... from ...` is not Lymra syntax",
        Label::new(span, "an import is one dotted path"),
        Help::replace("write the module path, then the item", span, rewritten),
    )
}

pub(super) fn assignment_in_condition(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        "`=` assigns a value; it does not compare",
        Label::new(span, "a condition needs a comparison here"),
        Help::replace("write `==` to compare", span, "=="),
    )
}

pub(super) fn chained_comparison(span: Span, rewritten: String) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0306,
        "comparisons cannot be chained",
        Label::new(
            span,
            "this compares the result of a comparison with another value",
        ),
        Help::replace(
            "compare each pair and join them with `and`",
            span,
            rewritten,
        ),
    )
}

pub(super) fn misplaced_with(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0307,
        "a `with` block has to follow a call directly",
        Label::new(span, "no call ends right before this `with`"),
        Help::text("give the block to a call, as in `filter(numbers) with n`"),
    )
}

pub(super) fn invalid_assignment_target(target: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0308,
        "cannot assign to this expression",
        Label::new(target, "not a name, a field or an element"),
        Help::text(
            "assign to `name`, `item.field` or `items[i]`; to compare two values, write `==`",
        ),
    )
}

pub(super) fn invalid_mutable_argument(argument: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0308,
        "`variable` has to mark a binding the call may change",
        Label::new(argument, "not a name, a field or an element"),
        Help::text("pass a binding, as in `add_item(variable numbers, 7)`, or remove `variable`"),
    )
}

pub(super) fn unavailable_feature(span: Span, feature: &str) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0309,
        format!("{feature} are not available in edition 0.1"),
        Label::new(span, "reserved for a later edition"),
        Help::text("remove it for now; the roadmap in the README says when it is planned"),
    )
}

pub(super) fn stray_else(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        "`else` without a matching `if`",
        Label::new(span, "no `if` block ends right above this line"),
        Help::text("put `else` right after the block of an `if`, indented like that `if`"),
    )
}

pub(super) fn stray_case(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0301,
        "`case` outside a `match`",
        Label::new(span, "a `case` belongs to the `match` one level above it"),
        Help::text("write the cases inside a `match`, indented one level"),
    )
}
