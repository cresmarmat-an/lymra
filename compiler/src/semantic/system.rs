/*!
 * Calls into the operating system (spec 5.1): reading and writing files,
 * and reading standard input. Each can fail with `FileError`, a choice the
 * language declares itself, so a program handles it like any other.
 */

use std::sync::Arc;

use super::expressions::CheckedCall;
use super::{CheckResult, Checker, errors};
use crate::ast::{self, ArgumentKind};
use crate::diagnostics::ErrorReported;
use crate::ir::{self, ChoiceType, CustomType, SystemOperation, Type};
use crate::source::Span;

/**
 * The variants of `FileError`, in the order of the codes the runtime gives
 * for them: nothing at the path, not allowed there, bytes that are not
 * UTF-8, standard input that has nothing left, and anything else the
 * system reports.
 */
pub(super) const FILE_ERROR_VARIANTS: [&str; 5] = [
    "NoSuchFile",
    "NoPermission",
    "NotText",
    "EndOfInput",
    "SystemError",
];

/// A choice the language declares itself, which no program can declare again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BuiltInChoice {
    /// What reading or writing a file or standard input fails with.
    FileError,
}

impl BuiltInChoice {
    pub const ALL: [BuiltInChoice; 1] = [BuiltInChoice::FileError];

    pub const fn name(self) -> &'static str {
        match self {
            Self::FileError => "FileError",
        }
    }

    pub const fn variants(self) -> &'static [&'static str] {
        match self {
            Self::FileError => &FILE_ERROR_VARIANTS,
        }
    }

    /// The choice, whose variants hold no fields.
    pub fn ty(self) -> Type {
        Type::Choice(Arc::new(ChoiceType {
            name: self.name().to_owned(),
            variants: self
                .variants()
                .iter()
                .map(|&name| CustomType {
                    name: name.to_owned(),
                    fields: Vec::new(),
                })
                .collect(),
            result: false,
        }))
    }

    /// Whether `name` is this choice or one of its variants.
    pub fn names(self, name: &str) -> bool {
        self.name() == name || self.variants().contains(&name)
    }
}

impl<'src> Checker<'src, '_> {
    /**
     * A call to `read_file`, `write_file`, `read_input` or `read_line`. Its
     * arguments are plain `string` values, and it gives a result: the text
     * read, or nothing for `write_file`, or a `FileError`.
     */
    pub(super) fn check_system(
        &mut self,
        operation: SystemOperation,
        call: &ast::Call<'src>,
        span: Span,
    ) -> CheckResult<CheckedCall<'src>> {
        let parameters = operation.parameters();
        if call.arguments.len() != parameters.len() {
            self.check_arguments_for_errors(&call.arguments);
            let arguments = Span::new(call.callee.span.end, span.end);
            return Err(self.report(errors::built_in_argument_count(
                operation.function(),
                parameters,
                call.arguments.len(),
                arguments,
            )));
        }
        let mut arguments = Vec::with_capacity(parameters.len());
        let mut failed = false;
        for argument in &call.arguments {
            let ArgumentKind::Positional(value) = &argument.kind else {
                self.mark_argument_reads(&call.arguments);
                return Err(self.report(errors::marked_built_in_argument(
                    operation.function(),
                    argument.span,
                )));
            };
            match self.check_expr(value, Some(&Type::String)) {
                Ok(value) => arguments.push(value),
                Err(ErrorReported) => failed = true,
            }
        }
        if failed {
            return Err(ErrorReported);
        }
        let value = operation.gives_text().then_some(Type::String);
        let result = ChoiceType::result(value, BuiltInChoice::FileError.ty());
        Ok(CheckedCall::Value(ir::Expr {
            kind: ir::ExprKind::System {
                operation,
                arguments,
            },
            ty: Type::Choice(Arc::new(result)),
            span,
        }))
    }
}
