/*!
 * Indentation tracking: turns leading whitespace into block structure.
 *
 * A file indents with tabs or with spaces, never both, and the first
 * indented line fixes the width of one level (spec 2.4). Every later line is
 * measured in levels against that unit.
 */

use super::errors;
use crate::diagnostics::Diagnostic;
use crate::source::Span;

/// Spaces assumed per level when a line contradicts the file's style and no width is known.
const FALLBACK_WIDTH: u32 = 4;

/// The leading whitespace of one line.
#[derive(Clone, Copy)]
pub(super) struct Indentation {
    pub spaces: u32,
    pub tabs: u32,
    pub span: Span,
}

#[derive(Clone, Copy)]
enum IndentUnit {
    Tabs,
    Spaces(u32),
}

/// The unit a file indents with, and the line that established it.
#[derive(Clone, Copy)]
struct IndentStyle {
    unit: IndentUnit,
    origin: Span,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum LayoutChange {
    Unchanged,
    Indented,
    Dedented(usize),
}

pub(super) struct Layout {
    style: Option<IndentStyle>,
    /// Indentation level of every open block, outermost first.
    open_levels: Vec<u32>,
}

impl Layout {
    pub fn new() -> Self {
        Self {
            style: None,
            open_levels: Vec::new(),
        }
    }

    /**
     * Applies the indentation of a new line and reports how the block depth
     * changed. `block_expected` says whether the line above opens a block. It
     * decides where a line with unusable whitespace goes, so that one
     * indentation mistake is one error and not also a missing or stray block.
     */
    pub fn apply(
        &mut self,
        indentation: Indentation,
        block_expected: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> LayoutChange {
        let reported_before = diagnostics.len();
        let measured = self.level_of(indentation, diagnostics);
        let whitespace_rejected = diagnostics.len() > reported_before;
        let current = self.current_level();
        let level = match (whitespace_rejected && measured > current, block_expected) {
            (true, true) => current + 1,
            (true, false) => current,
            (false, _) => measured,
        };

        if level > current {
            // Without a block to open, the parser reports the indentation itself as unexpected.
            if level > current + 1 && block_expected {
                let expected = self.whitespace_for(current + 1);
                diagnostics.push(errors::indented_too_deep(
                    indentation.span,
                    level - current,
                    expected,
                ));
            }
            self.open_levels.push(level);
            return LayoutChange::Indented;
        }

        /*
         * Close every block deeper than this line. A level that falls between
         * two open blocks can only follow a line already reported as too deep,
         * so the line stays in the inner block instead of raising a second error.
         */
        let mut closed = 0;
        while self.current_level() > level && self.level_below_top() >= level {
            self.open_levels.pop();
            closed += 1;
        }
        if closed == 0 {
            LayoutChange::Unchanged
        } else {
            LayoutChange::Dedented(closed)
        }
    }

    /// Closes every open block at the end of the file and returns how many there were.
    pub fn close_all(&mut self) -> usize {
        let open = self.open_levels.len();
        self.open_levels.clear();
        open
    }

    fn current_level(&self) -> u32 {
        self.open_levels.last().copied().unwrap_or_default()
    }

    fn level_below_top(&self) -> u32 {
        self.open_levels
            .iter()
            .rev()
            .nth(1)
            .copied()
            .unwrap_or_default()
    }

    fn level_of(&mut self, indentation: Indentation, diagnostics: &mut Vec<Diagnostic>) -> u32 {
        let Indentation { spaces, tabs, span } = indentation;
        if spaces == 0 && tabs == 0 {
            return 0;
        }
        if spaces > 0 && tabs > 0 {
            diagnostics.push(errors::tabs_and_spaces_on_one_line(span));
            return self.estimated_level(indentation);
        }

        let first_unit = if tabs > 0 {
            IndentUnit::Tabs
        } else {
            IndentUnit::Spaces(spaces)
        };
        let style = *self.style.get_or_insert(IndentStyle {
            unit: first_unit,
            origin: span,
        });
        match style.unit {
            IndentUnit::Tabs if tabs > 0 => tabs,
            IndentUnit::Spaces(width) if spaces > 0 => {
                if spaces % width != 0 {
                    diagnostics.push(errors::indentation_width(span, spaces, width, style.origin));
                }
                // Round to the nearest level, so one bad line does not shift the blocks after it.
                ((spaces + width / 2) / width).max(1)
            }
            IndentUnit::Tabs => {
                let level = self.estimated_level(indentation);
                let expected = self.whitespace_for(level);
                diagnostics.push(errors::indentation_style_conflict(
                    span,
                    "spaces",
                    "tabs",
                    style.origin,
                    expected,
                ));
                level
            }
            IndentUnit::Spaces(_) => {
                let expected = self.whitespace_for(tabs);
                diagnostics.push(errors::indentation_style_conflict(
                    span,
                    "tabs",
                    "spaces",
                    style.origin,
                    expected,
                ));
                tabs
            }
        }
    }

    /// A best guess at the level of a line whose whitespace breaks the file's style.
    fn estimated_level(&self, indentation: Indentation) -> u32 {
        let width = match self.style.map(|style| style.unit) {
            Some(IndentUnit::Spaces(width)) => width,
            Some(IndentUnit::Tabs) | None => FALLBACK_WIDTH,
        };
        indentation.tabs + indentation.spaces.div_ceil(width)
    }

    /// The whitespace that indents a line to `level` in this file's style.
    fn whitespace_for(&self, level: u32) -> String {
        match self.style.map(|style| style.unit) {
            Some(IndentUnit::Tabs) => "\t".repeat(level as usize),
            Some(IndentUnit::Spaces(width)) => " ".repeat((level * width) as usize),
            None => " ".repeat((level * FALLBACK_WIDTH) as usize),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spaces(count: u32) -> Indentation {
        Indentation {
            spaces: count,
            tabs: 0,
            span: Span::new(0, count),
        }
    }

    fn tabs(count: u32) -> Indentation {
        Indentation {
            spaces: 0,
            tabs: count,
            span: Span::new(0, count),
        }
    }

    /// A layout with one block open, in a file that indents by four spaces.
    fn four_space_layout() -> (Layout, Vec<Diagnostic>) {
        let mut layout = Layout::new();
        let mut diagnostics = Vec::new();
        assert_eq!(
            layout.apply(spaces(4), true, &mut diagnostics),
            LayoutChange::Indented
        );
        (layout, diagnostics)
    }

    #[test]
    fn the_first_indented_line_sets_the_width() {
        let mut layout = Layout::new();
        let mut diagnostics = Vec::new();
        assert_eq!(
            layout.apply(spaces(0), false, &mut diagnostics),
            LayoutChange::Unchanged
        );
        assert_eq!(
            layout.apply(spaces(2), true, &mut diagnostics),
            LayoutChange::Indented
        );
        assert_eq!(
            layout.apply(spaces(4), true, &mut diagnostics),
            LayoutChange::Indented
        );
        assert_eq!(
            layout.apply(spaces(2), false, &mut diagnostics),
            LayoutChange::Dedented(1)
        );
        assert_eq!(
            layout.apply(spaces(0), false, &mut diagnostics),
            LayoutChange::Dedented(1)
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn one_line_can_close_several_blocks() {
        let mut layout = Layout::new();
        let mut diagnostics = Vec::new();
        for count in [1, 2, 3] {
            assert_eq!(
                layout.apply(tabs(count), true, &mut diagnostics),
                LayoutChange::Indented
            );
        }
        assert_eq!(
            layout.apply(tabs(0), false, &mut diagnostics),
            LayoutChange::Dedented(3)
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn a_width_between_two_levels_goes_where_the_line_above_points() {
        let (mut layout, mut diagnostics) = four_space_layout();
        // No block is expected, so the line stays in the current block.
        assert_eq!(
            layout.apply(spaces(6), false, &mut diagnostics),
            LayoutChange::Unchanged
        );
        assert_eq!(diagnostics.len(), 1);
        // A block is expected, so the line opens it.
        assert_eq!(
            layout.apply(spaces(6), true, &mut diagnostics),
            LayoutChange::Indented
        );
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn a_width_between_two_outer_levels_closes_to_the_nearest() {
        let (mut layout, mut diagnostics) = four_space_layout();
        assert_eq!(
            layout.apply(spaces(8), true, &mut diagnostics),
            LayoutChange::Indented
        );
        assert_eq!(
            layout.apply(spaces(5), false, &mut diagnostics),
            LayoutChange::Dedented(1)
        );
        assert_eq!(
            layout.apply(spaces(1), false, &mut diagnostics),
            LayoutChange::Unchanged
        );
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn switching_between_tabs_and_spaces_is_reported_with_the_fix() {
        let (mut layout, mut diagnostics) = four_space_layout();
        assert_eq!(
            layout.apply(tabs(1), false, &mut diagnostics),
            LayoutChange::Unchanged
        );
        assert_eq!(diagnostics.len(), 1);
        let replacement = diagnostics[0]
            .help
            .suggestion
            .as_ref()
            .map(|edit| edit.replacement.as_str());
        assert_eq!(replacement, Some("    "));
    }

    #[test]
    fn a_block_two_levels_deeper_is_reported_and_closes_cleanly() {
        let (mut layout, mut diagnostics) = four_space_layout();
        assert_eq!(
            layout.apply(spaces(12), true, &mut diagnostics),
            LayoutChange::Indented
        );
        assert_eq!(diagnostics.len(), 1);
        // The level in between belongs to no open block. It must not raise a second error.
        assert_eq!(
            layout.apply(spaces(8), false, &mut diagnostics),
            LayoutChange::Unchanged
        );
        assert_eq!(
            layout.apply(spaces(4), false, &mut diagnostics),
            LayoutChange::Dedented(1)
        );
        assert_eq!(
            layout.apply(spaces(0), false, &mut diagnostics),
            LayoutChange::Dedented(1)
        );
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn deeper_indentation_with_no_block_to_open_is_left_to_the_parser() {
        let (mut layout, mut diagnostics) = four_space_layout();
        assert_eq!(
            layout.apply(spaces(12), false, &mut diagnostics),
            LayoutChange::Indented
        );
        assert!(diagnostics.is_empty());
    }
}
