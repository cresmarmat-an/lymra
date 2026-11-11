//! Source text and positions within it.

use std::ops::Range;

/// A half-open range of byte offsets into one source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// A zero-width span, used for insertion points and end-of-input markers.
    pub const fn empty(offset: u32) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    /// The smallest span that covers both `self` and `other`.
    #[must_use]
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }

    pub const fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// A one-based line and column for display. Columns count characters, not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

/// The reason a source file was refused before any analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceTooLarge;

/// One source file, with the line table needed to turn offsets into positions.
#[derive(Debug)]
pub struct SourceFile {
    name: String,
    text: String,
    line_starts: Vec<u32>,
}

impl SourceFile {
    /// Largest accepted source size in bytes, so every offset fits in a `u32`.
    pub const MAX_BYTES: usize = u32::MAX as usize - 1;

    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Result<Self, SourceTooLarge> {
        let text = text.into();
        if text.len() > Self::MAX_BYTES {
            return Err(SourceTooLarge);
        }
        // The size check above makes every index fit in a `u32`.
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(index, _)| index as u32 + 1))
            .collect();
        Ok(Self {
            name: name.into(),
            text,
            line_starts,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The offset one past the last byte.
    pub fn end(&self) -> u32 {
        self.text.len() as u32
    }

    /// The text a span covers, or an empty string for a span outside the file.
    pub fn slice(&self, span: Span) -> &str {
        self.text.get(span.range()).unwrap_or_default()
    }

    /// Zero-based index of the line that contains `offset`.
    pub fn line_index(&self, offset: u32) -> usize {
        self.line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1)
    }

    /// The span of a line's content, without its line break.
    pub fn line_span(&self, line_index: usize) -> Span {
        let Some(&start) = self.line_starts.get(line_index) else {
            return Span::empty(self.end());
        };
        let next_start = self
            .line_starts
            .get(line_index + 1)
            .copied()
            .unwrap_or(self.end());
        let content = self
            .slice(Span::new(start, next_start))
            .trim_end_matches(['\n', '\r']);
        Span::new(start, start + content.len() as u32)
    }

    pub fn line_text(&self, line_index: usize) -> &str {
        self.slice(self.line_span(line_index))
    }

    pub fn position(&self, offset: u32) -> Position {
        let line_index = self.line_index(offset);
        let line_start = self
            .line_starts
            .get(line_index)
            .copied()
            .unwrap_or_default();
        let column = self.slice(Span::new(line_start, offset)).chars().count() + 1;
        Position {
            line: line_index + 1,
            column,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> SourceFile {
        SourceFile::new("test.lym", text).expect("test sources are small")
    }

    #[test]
    fn positions_are_one_based_and_count_characters() {
        let source = file("ab\né = 1\n");
        assert_eq!(source.position(0), Position { line: 1, column: 1 });
        assert_eq!(source.position(1), Position { line: 1, column: 2 });
        // `é` is two bytes but one column.
        assert_eq!(source.position(5), Position { line: 2, column: 2 });
    }

    #[test]
    fn line_text_excludes_any_line_break() {
        let source = file("one\r\ntwo\nthree");
        assert_eq!(source.line_text(0), "one");
        assert_eq!(source.line_text(1), "two");
        assert_eq!(source.line_text(2), "three");
        assert_eq!(source.line_text(3), "");
    }

    #[test]
    fn offset_at_end_of_file_is_on_the_last_line() {
        let source = file("a\nb");
        assert_eq!(
            source.position(source.end()),
            Position { line: 2, column: 2 }
        );
        let trailing = file("a\n");
        assert_eq!(
            trailing.position(trailing.end()),
            Position { line: 2, column: 1 }
        );
    }

    #[test]
    fn slice_outside_the_file_is_empty() {
        let source = file("abc");
        assert_eq!(source.slice(Span::new(2, 90)), "");
        assert_eq!(source.slice(Span::new(1, 3)), "bc");
    }

    #[test]
    fn spans_join_to_their_cover() {
        assert_eq!(Span::new(4, 6).to(Span::new(1, 5)), Span::new(1, 6));
        assert!(Span::empty(3).is_empty());
    }
}
