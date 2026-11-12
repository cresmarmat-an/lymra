//! Compiler errors as data: what went wrong, where, and how to fix it.

mod codes;
mod render;

pub use codes::ErrorCode;
pub use render::{render, render_location, render_report};

use crate::source::Span;

/**
 * Proof that a diagnostic describing a failure has been recorded.
 *
 * Functions return it instead of the diagnostic itself, so a failure can
 * propagate with `?` without being reported a second time.
 */
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorReported;

/// One rejected construct: an error code, the reason, the place, and a fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: ErrorCode,
    pub message: String,
    pub primary: Label,
    pub secondary: Vec<Label>,
    pub help: Help,
}

impl Diagnostic {
    /// A fix is required, not optional: every rejected program gets one (spec goal 3).
    pub fn new(code: ErrorCode, message: impl Into<String>, primary: Label, help: Help) -> Self {
        Self {
            code,
            message: message.into(),
            primary,
            secondary: Vec::new(),
            help,
        }
    }

    /// Adds a second place that explains the error, such as where a rule was set.
    #[must_use]
    pub fn with_secondary(mut self, label: Label) -> Self {
        self.secondary.push(label);
        self
    }
}

/// A source range with a short note on its role in the error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub text: String,
}

impl Label {
    pub fn new(span: Span, text: impl Into<String>) -> Self {
        Self {
            span,
            text: text.into(),
        }
    }
}

/// The concrete fix a diagnostic carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Help {
    pub message: String,
    pub suggestion: Option<Suggestion>,
}

impl Help {
    /// A fix described in words, for cases the compiler cannot rewrite by itself.
    pub fn text(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            suggestion: None,
        }
    }

    /// A fix that replaces the source at `span` with `replacement`.
    pub fn replace(message: impl Into<String>, span: Span, replacement: impl Into<String>) -> Self {
        let suggestion = Suggestion {
            span,
            replacement: replacement.into(),
        };
        Self {
            message: message.into(),
            suggestion: Some(suggestion),
        }
    }

    /// A fix that inserts `text` at `offset`.
    pub fn insert(message: impl Into<String>, offset: u32, text: impl Into<String>) -> Self {
        Self::replace(message, Span::empty(offset), text)
    }

    /// A fix that deletes the source at `span`.
    pub fn remove(message: impl Into<String>, span: Span) -> Self {
        Self::replace(message, span, "")
    }
}

/// A source edit that applies a fix. Rendered only when it stays on one line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub span: Span,
    pub replacement: String,
}
