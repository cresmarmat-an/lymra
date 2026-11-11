//! Renders diagnostics as text, in the format section 11 of the spec defines.

use super::{Diagnostic, Label, Suggestion};
use crate::source::{SourceFile, Span};

/// Columns a tab takes up in a rendered source line.
const TAB_WIDTH: usize = 4;

const PRIMARY_MARKER: char = '^';
const SECONDARY_MARKER: char = '-';

/// A label placed on its source line, measured in display columns.
struct Underline<'a> {
    line: usize,
    column: usize,
    width: usize,
    marker: char,
    text: &'a str,
}

/// Collects output lines, trimming trailing spaces so the result is stable.
struct Canvas {
    text: String,
    gutter: usize,
}

impl Canvas {
    fn push(&mut self, line: &str) {
        self.text.push_str(line.trim_end());
        self.text.push('\n');
    }

    fn empty_gutter(&mut self) {
        self.push(&format!("{:gutter$} |", "", gutter = self.gutter));
    }

    fn source_line(&mut self, line_index: usize, text: &str) {
        let number = line_index + 1;
        self.push(&format!(
            "{number:>gutter$} | {}",
            expand_tabs(text),
            gutter = self.gutter
        ));
    }

    fn underline(&mut self, underline: &Underline<'_>) {
        let markers = underline.marker.to_string().repeat(underline.width);
        let line = format!(
            "{:gutter$} | {:column$}{markers} {}",
            "",
            "",
            underline.text,
            gutter = self.gutter,
            column = underline.column,
        );
        self.push(&line);
    }
}

/// Renders one diagnostic: header, location, annotated source, and the fix.
pub fn render(diagnostic: &Diagnostic, source: &SourceFile) -> String {
    let underlines = underlines(diagnostic, source);
    let fixed_line = diagnostic
        .help
        .suggestion
        .as_ref()
        .and_then(|edit| fixed_line(edit, source));

    let fixed_line_index = fixed_line.as_ref().map(|(line, _)| *line);
    let last_line = underlines
        .iter()
        .map(|underline| underline.line)
        .chain(fixed_line_index)
        .max();
    let gutter = (last_line.unwrap_or_default() + 1).to_string().len();
    let position = source.position(diagnostic.primary.span.start);

    let mut canvas = Canvas {
        text: String::new(),
        gutter,
    };
    canvas.push(&format!(
        "error[{}]: {}",
        diagnostic.code, diagnostic.message
    ));
    let location = format!("{}:{}:{}", source.name(), position.line, position.column);
    canvas.push(&format!("{:gutter$}--> {location}", ""));
    canvas.empty_gutter();

    let mut previous_line: Option<usize> = None;
    for underline in &underlines {
        if previous_line != Some(underline.line) {
            if previous_line.is_some_and(|previous| underline.line > previous + 1) {
                canvas.push("...");
            }
            canvas.source_line(underline.line, source.line_text(underline.line));
            previous_line = Some(underline.line);
        }
        canvas.underline(underline);
    }

    canvas.empty_gutter();
    canvas.push(&format!("help: {}", diagnostic.help.message));
    if let Some((line, text)) = fixed_line {
        canvas.empty_gutter();
        canvas.source_line(line, &text);
    }
    canvas.text
}

/// Renders every diagnostic, then a summary that points at `lymra explain`.
pub fn render_report(diagnostics: &[Diagnostic], source: &SourceFile) -> String {
    let mut report = String::new();
    for diagnostic in diagnostics {
        report.push_str(&render(diagnostic, source));
        report.push('\n');
    }
    if let Some(first) = diagnostics.first() {
        let count = diagnostics.len();
        let noun = if count == 1 { "error" } else { "errors" };
        report.push_str(&format!("found {count} {noun} in {}\n", source.name()));
        report.push_str(&format!(
            "run `lymra explain {}` to read why a rule exists and how to fix it\n",
            first.code
        ));
    }
    report
}

/// The primary label first, then the secondary ones, grouped by source line.
fn underlines<'a>(diagnostic: &'a Diagnostic, source: &SourceFile) -> Vec<Underline<'a>> {
    let primary = underline(&diagnostic.primary, PRIMARY_MARKER, source);
    let secondary = diagnostic
        .secondary
        .iter()
        .map(|label| underline(label, SECONDARY_MARKER, source));
    let mut all: Vec<Underline<'a>> = std::iter::once(primary).chain(secondary).collect();
    all.sort_by_key(|underline| underline.line);
    all
}

/// Places a label on the line where it starts. A span that runs past the line is cut there.
fn underline<'a>(label: &'a Label, marker: char, source: &SourceFile) -> Underline<'a> {
    let line = source.line_index(label.span.start);
    let line_span = source.line_span(line);
    let start = label.span.start.clamp(line_span.start, line_span.end);
    let end = label.span.end.clamp(start, line_span.end);
    Underline {
        line,
        column: display_width(source.slice(Span::new(line_span.start, start))),
        width: display_width(source.slice(Span::new(start, end))).max(1),
        marker,
        text: &label.text,
    }
}

/**
 * The source line as it would read with the suggestion applied.
 * Returns `None` for edits that span lines or leave the line blank, where
 * showing one rewritten line would mislead.
 */
fn fixed_line(suggestion: &Suggestion, source: &SourceFile) -> Option<(usize, String)> {
    let line = source.line_index(suggestion.span.start);
    let line_span = source.line_span(line);
    if suggestion.span.end > line_span.end || suggestion.replacement.contains('\n') {
        return None;
    }
    let before = source.slice(Span::new(line_span.start, suggestion.span.start));
    let after = source.slice(Span::new(suggestion.span.end, line_span.end));
    let fixed = format!("{before}{}{after}", suggestion.replacement);
    (!fixed.trim().is_empty()).then_some((line, fixed))
}

fn expand_tabs(text: &str) -> String {
    text.replace('\t', &" ".repeat(TAB_WIDTH))
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| if character == '\t' { TAB_WIDTH } else { 1 })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{ErrorCode, Help};

    fn file(text: &str) -> SourceFile {
        SourceFile::new("src/main.lym", text).expect("test sources are small")
    }

    /// Finds the span of the first occurrence of `needle`.
    fn span_of(source: &SourceFile, needle: &str) -> Span {
        let start = source
            .text()
            .find(needle)
            .expect("needle is in the test source") as u32;
        Span::new(start, start + needle.len() as u32)
    }

    #[test]
    fn matches_the_layout_in_the_spec() {
        let source = file(&format!("{}    string total = 0\n", "\n".repeat(11)));
        let target = span_of(&source, "string total");
        let diagnostic = Diagnostic::new(
            ErrorCode::L0203,
            "this declaration needs `variable` or `constant` in front",
            Label::new(target, "a declaration starts with a keyword"),
            Help::insert("declare it with a keyword", target.start, "variable "),
        );
        let expected = "\
error[L0203]: this declaration needs `variable` or `constant` in front
  --> src/main.lym:12:5
   |
12 |     string total = 0
   |     ^^^^^^^^^^^^ a declaration starts with a keyword
   |
help: declare it with a keyword
   |
12 |     variable string total = 0
";
        assert_eq!(render(&diagnostic, &source), expected);
    }

    #[test]
    fn secondary_labels_show_their_own_lines_with_a_gap_marker() {
        let source = file("function main()\n    one()\n\n\n\ttwo()\n");
        let diagnostic = Diagnostic::new(
            ErrorCode::L0102,
            "mixed",
            Label::new(span_of(&source, "\t"), "tab here"),
            Help::text("use spaces"),
        )
        .with_secondary(Label::new(span_of(&source, "    "), "spaces start here"));
        let expected = "\
error[L0102]: mixed
 --> src/main.lym:5:1
  |
2 |     one()
  | ---- spaces start here
...
5 |     two()
  | ^^^^ tab here
  |
help: use spaces
";
        assert_eq!(render(&diagnostic, &source), expected);
    }

    #[test]
    fn a_span_at_the_end_of_a_line_still_gets_one_marker() {
        let source = file("print(\"hi\"\n");
        let end_of_line = Span::empty(source.line_span(0).end);
        let diagnostic = Diagnostic::new(
            ErrorCode::L0301,
            "unclosed",
            Label::new(end_of_line, "expected `)`"),
            Help::insert("add `)`", end_of_line.start, ")"),
        );
        let rendered = render(&diagnostic, &source);
        assert!(
            rendered.contains("1 | print(\"hi\"\n  |           ^ expected `)`\n"),
            "{rendered}"
        );
        assert!(rendered.ends_with("1 | print(\"hi\")\n"), "{rendered}");
    }

    #[test]
    fn an_edit_that_blanks_the_line_shows_only_the_message() {
        let source = file("#!strict\n");
        let whole_line = source.line_span(0);
        let diagnostic = Diagnostic::new(
            ErrorCode::L0105,
            "unknown directive",
            Label::new(whole_line, ""),
            Help::remove("remove this line", whole_line),
        );
        assert!(render(&diagnostic, &source).ends_with("help: remove this line\n"));
    }

    #[test]
    fn the_report_ends_with_a_count_and_a_pointer_to_explain() {
        let source = file("x\n");
        let diagnostic = Diagnostic::new(
            ErrorCode::L0301,
            "first",
            Label::new(Span::new(0, 1), ""),
            Help::text("fix it"),
        );
        let report = render_report(&[diagnostic.clone(), diagnostic], &source);
        assert!(report.contains("found 2 errors in src/main.lym\n"));
        assert!(
            report.ends_with(
                "run `lymra explain L0301` to read why a rule exists and how to fix it\n"
            )
        );
        assert_eq!(render_report(&[], &source), "");
    }
}
