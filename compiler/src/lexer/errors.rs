//! The wording of every error the lexer reports.

use super::number::NumberError;
use super::text::MAX_INTERPOLATION_DEPTH;
use crate::diagnostics::{Diagnostic, ErrorCode, Help, Label};
use crate::source::Span;

fn code_point(character: char) -> String {
    format!("U+{:04X}", u32::from(character))
}

pub(super) fn unexpected_character(character: char, span: Span) -> Diagnostic {
    let name = if character.is_control() {
        format!("control character {}", code_point(character))
    } else {
        format!("character `{character}`")
    };
    Diagnostic::new(
        ErrorCode::L0108,
        format!("unexpected {name}"),
        Label::new(span, "this character has no meaning here"),
        Help::remove("remove it", span),
    )
}

/// Reports a space-like or zero-width character that looks like nothing in an editor.
pub(super) fn invisible_character(character: char, span: Span) -> Diagnostic {
    let (label, help) = if character.is_whitespace() {
        (
            "this is not a normal space",
            Help::replace("replace it with a normal space", span, " "),
        )
    } else {
        (
            "a zero-width character is hiding here",
            Help::remove("remove it", span),
        )
    };
    Diagnostic::new(
        ErrorCode::L0108,
        format!(
            "invisible character {} in the source",
            code_point(character)
        ),
        Label::new(span, label),
        help,
    )
}

pub(super) fn non_ascii_name(name: &str, span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0108,
        format!("`{name}` contains a character that names cannot use"),
        Label::new(span, "names use only a-z, A-Z, 0-9 and `_`"),
        Help::text("rename it using those characters only"),
    )
}

pub(super) fn foreign_operator(
    found: &str,
    lymra: &str,
    span: Span,
    replacement: &str,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        format!("`{found}` is not a Lymra operator"),
        Label::new(span, format!("Lymra spells this `{lymra}`")),
        Help::replace(format!("write `{lymra}`"), span, replacement),
    )
}

pub(super) fn foreign_line_comment(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        "`//` does not start a comment in Lymra",
        Label::new(
            span,
            "the rest of this line was skipped as if it were a comment",
        ),
        Help::replace("start the comment with `#`", span, "#"),
    )
}

pub(super) fn foreign_block_comment(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0305,
        "`/*` does not start a comment in Lymra",
        Label::new(span, "this comment was skipped"),
        Help::text("write a multiline comment as `#[ ... ]#`"),
    )
}

pub(super) fn foreign_brace(brace: char, span: Span) -> Diagnostic {
    let action = if brace == '{' { "open" } else { "close" };
    Diagnostic::new(
        ErrorCode::L0305,
        format!("`{brace}` does not {action} a block in Lymra"),
        Label::new(span, "blocks are defined by indentation"),
        Help::remove("remove the braces and indent the lines of the block", span),
    )
}

/// `replacement` is the literal rewritten with double quotes, when that needs no other change.
pub(super) fn single_quoted_text(span: Span, replacement: Option<String>) -> Diagnostic {
    let help = match replacement {
        Some(replacement) => Help::replace("use double quotes", span, replacement),
        None => {
            Help::text("use double quotes, and write `\\\"` for a double quote inside the text")
        }
    };
    Diagnostic::new(
        ErrorCode::L0305,
        "text is written with double quotes",
        Label::new(span, "single quotes are not Lymra syntax"),
        help,
    )
}

pub(super) fn unterminated_text(opening_quote: Span, line_end: u32) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0111,
        "this text is never closed",
        Label::new(
            opening_quote,
            "the text starts here and has to end on the same line",
        ),
        Help::insert("add the closing `\"`", line_end, "\""),
    )
}

pub(super) fn unterminated_interpolation(opening_brace: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0111,
        "this interpolation is never closed",
        Label::new(opening_brace, "`{` starts an interpolation here"),
        Help::text("close it with `}`, or write `{{` if you want a literal brace"),
    )
}

pub(super) fn unknown_escape(escaped: char, span: Span) -> Diagnostic {
    let help = match escaped {
        '{' | '}' => {
            let doubled = format!("{escaped}{escaped}");
            Help::replace(
                format!("write `{doubled}` for a literal brace"),
                span,
                doubled,
            )
        }
        '\'' => Help::replace("a single quote needs no backslash", span, "'"),
        _ => Help::replace(
            "write `\\\\` for a backslash; the escapes are `\\n` `\\t` `\\r` `\\0` `\\\\` `\\\"`",
            span,
            format!("\\\\{escaped}"),
        ),
    };
    Diagnostic::new(
        ErrorCode::L0112,
        format!("unknown escape `\\{escaped}`"),
        Label::new(span, "not an escape Lymra knows"),
        help,
    )
}

pub(super) fn unmatched_brace(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0113,
        "`}` without a matching `{` in text",
        Label::new(
            span,
            "a single `}` closes an interpolation, and none is open",
        ),
        Help::replace("write `}}` for a literal brace", span, "}}"),
    )
}

pub(super) fn interpolation_too_deep(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0310,
        "text is nested too deeply inside interpolations",
        Label::new(
            span,
            format!("more than {MAX_INTERPOLATION_DEPTH} levels of text within text"),
        ),
        Help::text("build the inner text first and store it in a constant"),
    )
}

pub(super) fn invalid_number(error: NumberError, span: Span) -> Diagnostic {
    let help = match error.replacement {
        Some(replacement) => Help::replace(error.help, span, replacement),
        None => Help::text(error.help),
    };
    Diagnostic::new(ErrorCode::L0114, error.message, Label::new(span, ""), help)
}

pub(super) fn unterminated_comment(opening: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0109,
        "this multiline comment is never closed",
        Label::new(
            opening,
            "the comment starts here and runs to the end of the file",
        ),
        Help::text(
            "close it with `]#`; for a one-line comment that begins with `[`, put a space after `#`",
        ),
    )
}

pub(super) fn code_after_comment(code: Span, closing: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0110,
        "code follows a multiline comment on the same line",
        Label::new(code, "this code is easy to miss"),
        Help::text("move the code to its own line"),
    )
    .with_secondary(Label::new(closing, "the multiline comment ends here"))
}

pub(super) fn tabs_and_spaces_on_one_line(span: Span) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0102,
        "this line is indented with both tabs and spaces",
        Label::new(span, "tabs and spaces together"),
        Help::text("indent with tabs only or with spaces only, the same way in the whole file"),
    )
}

/// `found` and `expected` are the words "tabs" and "spaces", in the order that applies.
pub(super) fn indentation_style_conflict(
    span: Span,
    found: &str,
    expected: &str,
    origin: Span,
    replacement: String,
) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0102,
        format!("this line is indented with {found}, but the file indents with {expected}"),
        Label::new(span, format!("{found} here")),
        Help::replace(
            format!("indent this line with {expected}"),
            span,
            replacement,
        ),
    )
    .with_secondary(Label::new(
        origin,
        format!("the file starts indenting with {expected} here"),
    ))
}

pub(super) fn indentation_width(span: Span, found: u32, width: u32, origin: Span) -> Diagnostic {
    let lower = found / width * width;
    let upper = lower + width;
    let help = if lower == 0 {
        format!("use {upper} spaces, or none to leave the block")
    } else {
        format!("use {lower} or {upper} spaces")
    };
    Diagnostic::new(
        ErrorCode::L0107,
        format!("this line is indented by {found} spaces, which is not a multiple of {width}"),
        Label::new(span, format!("{found} spaces")),
        Help::text(help),
    )
    .with_secondary(Label::new(
        origin,
        format!("the first indented line sets the width to {width} spaces"),
    ))
}

pub(super) fn indented_too_deep(span: Span, levels_deeper: u32, replacement: String) -> Diagnostic {
    Diagnostic::new(
        ErrorCode::L0107,
        format!(
            "this line is indented {levels_deeper} levels deeper than the line that opens its block"
        ),
        Label::new(span, "too deep"),
        Help::replace("indent a block exactly one level deeper", span, replacement),
    )
}
