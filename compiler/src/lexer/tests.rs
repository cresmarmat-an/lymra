//! Lexer tests: token streams for valid code and one error per mistake for invalid code.

use super::*;
use crate::diagnostics::ErrorCode;
use TokenKind::{
    Assign, Comma, Dedent, Directive, Dot, EndOfFile, Equal, Error, Float, Greater, GreaterEqual,
    Identifier, Indent, Integer, InterpolationEnd, InterpolationStart, LeftParen, Less, LessEqual,
    Newline, NotEqual, RightParen, TextEnd, TextSegment, TextStart,
};

fn lex_text(text: &str) -> Lexed {
    let source = SourceFile::new("test.lym", text).expect("test sources are small");
    lex(&source)
}

/// Token kinds of a source that must lex without errors.
fn kinds(text: &str) -> Vec<TokenKind> {
    let lexed = lex_text(text);
    assert!(
        lexed.diagnostics.is_empty(),
        "unexpected errors for {text:?}: {:#?}",
        lexed.diagnostics
    );
    lexed.tokens.iter().map(|token| token.kind).collect()
}

/// Error codes reported for a source, in order.
fn codes(text: &str) -> Vec<ErrorCode> {
    lex_text(text)
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn keyword(keyword: Keyword) -> TokenKind {
    TokenKind::Keyword(keyword)
}

#[test]
fn an_empty_file_is_just_the_end_of_file() {
    assert_eq!(kinds(""), [EndOfFile]);
    assert_eq!(kinds("\n\n   \n# only a comment\n"), [EndOfFile]);
}

#[test]
fn words_are_names_or_keywords() {
    assert_eq!(
        kinds("variable count_2 = _"),
        [
            keyword(Keyword::Variable),
            Identifier,
            Assign,
            Identifier,
            Newline,
            EndOfFile
        ]
    );
}

#[test]
fn operators_use_the_longest_match() {
    assert_eq!(
        kinds("a <= b == c != d >= e < f > g"),
        [
            Identifier,
            LessEqual,
            Identifier,
            Equal,
            Identifier,
            NotEqual,
            Identifier,
            GreaterEqual,
            Identifier,
            Less,
            Identifier,
            Greater,
            Identifier,
            Newline,
            EndOfFile
        ]
    );
    assert_eq!(kinds("1 + 2 - 3 * 4 / 5 % 6").len(), 13);
}

#[test]
fn blocks_become_indent_and_dedent_tokens() {
    let source = "function main()\n    if ready\n        go()\n    stop()\n";
    assert_eq!(
        kinds(source),
        [
            keyword(Keyword::Function),
            Identifier,
            LeftParen,
            RightParen,
            Newline,
            Indent,
            keyword(Keyword::If),
            Identifier,
            Newline,
            Indent,
            Identifier,
            LeftParen,
            RightParen,
            Newline,
            Dedent,
            Identifier,
            LeftParen,
            RightParen,
            Newline,
            Dedent,
            EndOfFile
        ]
    );
}

#[test]
fn a_missing_final_line_break_still_ends_the_line_and_the_blocks() {
    assert_eq!(
        kinds("if a\n\tb"),
        [
            keyword(Keyword::If),
            Identifier,
            Newline,
            Indent,
            Identifier,
            Newline,
            Dedent,
            EndOfFile
        ]
    );
}

#[test]
fn blank_and_comment_lines_do_not_affect_blocks() {
    let source = "if a\n\n    # note\n    b\n  \n#[ block\n   comment ]#\n    c\n";
    assert_eq!(
        kinds(source),
        [
            keyword(Keyword::If),
            Identifier,
            Newline,
            Indent,
            Identifier,
            Newline,
            Identifier,
            Newline,
            Dedent,
            EndOfFile
        ]
    );
}

#[test]
fn line_breaks_inside_brackets_continue_the_statement() {
    let source = "call(\n    a,\n        b\n)\nnext\n";
    assert_eq!(
        kinds(source),
        [
            Identifier, LeftParen, Identifier, Comma, Identifier, RightParen, Newline, Identifier,
            Newline, EndOfFile
        ]
    );
}

#[test]
fn windows_line_breaks_are_accepted() {
    assert_eq!(
        kinds("if a\r\n    b\r\n"),
        [
            keyword(Keyword::If),
            Identifier,
            Newline,
            Indent,
            Identifier,
            Newline,
            Dedent,
            EndOfFile
        ]
    );
}

#[test]
fn a_shebang_and_a_byte_order_mark_are_skipped() {
    assert_eq!(
        kinds("#!/usr/bin/env lymra\nx\n"),
        [Identifier, Newline, EndOfFile]
    );
    assert_eq!(kinds("\u{FEFF}x\n"), [Identifier, Newline, EndOfFile]);
}

#[test]
fn a_directive_is_one_token_for_the_whole_line() {
    let lexed = lex_text("#!optimize speed   \nx\n");
    let kinds: Vec<TokenKind> = lexed.tokens.iter().map(|token| token.kind).collect();
    assert_eq!(kinds, [Directive, Newline, Identifier, Newline, EndOfFile]);
    assert_eq!(lexed.tokens[0].span, Span::new(0, 16));
}

#[test]
fn numbers_carry_their_values() {
    assert_eq!(
        kinds("42 0xFF 1_000 2.5"),
        [
            Integer(42),
            Integer(255),
            Integer(1000),
            Float(2.5),
            Newline,
            EndOfFile
        ]
    );
    assert_eq!(
        kinds("1.field"),
        [Integer(1), Dot, Identifier, Newline, EndOfFile]
    );
}

#[test]
fn text_is_a_balanced_run_of_segments_and_interpolations() {
    assert_eq!(
        kinds(r#""plain""#),
        [TextStart, TextSegment, TextEnd, Newline, EndOfFile]
    );
    assert_eq!(kinds(r#""""#), [TextStart, TextEnd, Newline, EndOfFile]);
    assert_eq!(
        kinds(r#""Hello, {name}!""#),
        [
            TextStart,
            TextSegment,
            InterpolationStart,
            Identifier,
            InterpolationEnd,
            TextSegment,
            TextEnd,
            Newline,
            EndOfFile
        ]
    );
}

#[test]
fn text_may_nest_inside_an_interpolation() {
    assert_eq!(
        kinds(r#""{greet("you")}""#),
        [
            TextStart,
            InterpolationStart,
            Identifier,
            LeftParen,
            TextStart,
            TextSegment,
            TextEnd,
            RightParen,
            InterpolationEnd,
            TextEnd,
            Newline,
            EndOfFile
        ]
    );
}

#[test]
fn doubled_braces_and_escapes_stay_inside_one_segment() {
    assert_eq!(
        kinds(r#""{{literal}} \"quoted\" \n""#),
        [TextStart, TextSegment, TextEnd, Newline, EndOfFile]
    );
}

#[test]
fn a_bracket_left_open_in_an_interpolation_does_not_swallow_the_next_line() {
    let lexed = lex_text("a = \"{f(1}\"\nb\n");
    let newlines = lexed
        .tokens
        .iter()
        .filter(|token| token.kind == Newline)
        .count();
    assert_eq!(newlines, 2);
}

#[test]
fn nested_multiline_comments_close_in_pairs() {
    assert_eq!(
        kinds("a #[ one #[ two ]# still comment ]# b"),
        [Identifier, Identifier, Newline, EndOfFile]
    );
}

#[test]
fn each_lexical_mistake_is_one_error() {
    let cases: &[(&str, ErrorCode)] = &[
        ("x = $", ErrorCode::L0108),
        ("x = caf\u{e9}", ErrorCode::L0108),
        ("x =\u{a0}1", ErrorCode::L0108),
        ("#[ never closed", ErrorCode::L0109),
        ("#[ a\n]# x = 1\n", ErrorCode::L0110),
        ("x = \"open", ErrorCode::L0111),
        ("x = \"{name\"", ErrorCode::L0111),
        ("x = \"a\\qb\"", ErrorCode::L0112),
        ("x = \"a}b\"", ErrorCode::L0113),
        ("x = 007", ErrorCode::L0114),
        ("x = 12abc", ErrorCode::L0114),
        ("if a\n    b\n\tc\n", ErrorCode::L0102),
        ("if a\n  \tb\n", ErrorCode::L0102),
        ("if a\n    b\n      c\n", ErrorCode::L0107),
        ("if a\n    if b\n            c\n", ErrorCode::L0107),
        ("if a && b", ErrorCode::L0305),
        ("if a || b", ErrorCode::L0305),
        ("if !a", ErrorCode::L0305),
        ("if a === b", ErrorCode::L0305),
        ("x = 1 // note", ErrorCode::L0305),
        ("x = 1 /* note */", ErrorCode::L0305),
        ("if a {\n    b\n}\n", ErrorCode::L0305),
        ("x = 'text'", ErrorCode::L0305),
    ];
    for (source, expected) in cases {
        assert_eq!(codes(source), [*expected], "for source {source:?}");
    }
}

#[test]
fn foreign_operators_are_read_as_their_lymra_keywords() {
    let lexed = lex_text("a && b || !c");
    let kinds: Vec<TokenKind> = lexed.tokens.iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        [
            Identifier,
            keyword(Keyword::And),
            Identifier,
            keyword(Keyword::Or),
            keyword(Keyword::Not),
            Identifier,
            Newline,
            EndOfFile
        ]
    );
    let replacements: Vec<&str> = lexed
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.help.suggestion.as_ref())
        .map(|suggestion| suggestion.replacement.as_str())
        .collect();
    assert_eq!(replacements, ["and", "or", "not "]);
}

#[test]
fn rejected_source_leaves_an_error_token_and_a_balanced_stream() {
    let lexed = lex_text("x = \"open {a");
    let kinds: Vec<TokenKind> = lexed.tokens.iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        [
            Identifier,
            Assign,
            TextStart,
            TextSegment,
            InterpolationStart,
            Identifier,
            Error,
            InterpolationEnd,
            Error,
            TextEnd,
            Newline,
            EndOfFile
        ]
    );
    assert_eq!(lexed.diagnostics.len(), 1);
}

#[test]
fn text_nested_past_the_limit_is_rejected_without_overflowing_the_stack() {
    let source = "\"{".repeat(100_000);
    let lexed = lex_text(&source);
    assert_eq!(codes(&source), [ErrorCode::L0310]);
    assert_eq!(lexed.tokens.last().map(|token| token.kind), Some(EndOfFile));
}

#[test]
fn every_token_span_lies_inside_the_source() {
    let source = "function main()\n    print(\"{a} \u{e9}\")\n\treturn 1 +\n";
    let lexed = lex_text(source);
    for token in &lexed.tokens {
        assert!(token.span.start <= token.span.end && token.span.end as usize <= source.len());
        assert!(source.is_char_boundary(token.span.start as usize));
        assert!(source.is_char_boundary(token.span.end as usize));
    }
}
