//! The bindings of the function being checked: its parameters, locals and loop variables.

use std::collections::HashMap;

use crate::ast::Name;
use crate::diagnostics::ErrorReported;
use crate::ir::{self, LocalId, Type};
use crate::source::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LocalKind {
    Constant,
    Variable,
    Parameter,
    VariableParameter,
    LoopVariable,
}

impl LocalKind {
    pub fn is_mutable(self) -> bool {
        matches!(self, Self::Variable | Self::VariableParameter)
    }

    /// What the binding is, as messages call it.
    pub fn noun(self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::Variable => "variable",
            Self::Parameter | Self::VariableParameter => "parameter",
            Self::LoopVariable => "loop variable",
        }
    }
}

#[derive(Debug)]
pub(super) struct Local<'src> {
    pub name: Name<'src>,
    /// The type, or `Err` when the declaration was rejected and every use is already explained.
    pub ty: Result<Type, ErrorReported>,
    pub kind: LocalKind,
    /**
     * The keyword a fix rewrites: `constant` or `variable` for a binding,
     * and the `variable ` before a parameter's type, including the space.
     */
    pub keyword: Option<Span>,
    pub read: bool,
    pub written: bool,
    /// Set when the declaration itself was rejected, so the binding is not reported again as unused.
    pub declaration_failed: bool,
}

/// What `break` and `continue` see while a loop body is checked.
#[derive(Debug, Default)]
struct Loop {
    has_break: bool,
}

#[derive(Debug)]
pub(super) struct FunctionScope<'src> {
    pub function_name: Name<'src>,
    /// The declared return type, `Ok(None)` for none, or `Err` when it could not be resolved.
    pub return_type: Result<Option<Type>, ErrorReported>,
    locals: Vec<Local<'src>>,
    /// Names visible at the current point. A name is never declared twice, so nothing is hidden.
    visible: HashMap<&'src str, LocalId>,
    /// The locals declared in each open block, innermost last.
    blocks: Vec<Vec<LocalId>>,
    loops: Vec<Loop>,
}

impl<'src> FunctionScope<'src> {
    pub fn new(
        function_name: Name<'src>,
        return_type: Result<Option<Type>, ErrorReported>,
    ) -> Self {
        Self {
            function_name,
            return_type,
            locals: Vec::new(),
            visible: HashMap::new(),
            blocks: vec![Vec::new()],
            loops: Vec::new(),
        }
    }

    pub fn lookup(&self, name: &str) -> Option<LocalId> {
        self.visible.get(name).copied()
    }

    pub fn local(&self, id: LocalId) -> Option<&Local<'src>> {
        self.locals.get(id)
    }

    pub fn local_mut(&mut self, id: LocalId) -> Option<&mut Local<'src>> {
        self.locals.get_mut(id)
    }

    pub fn visible_names(&self) -> impl Iterator<Item = &'src str> + '_ {
        self.visible.keys().copied()
    }

    /// Declares a binding in the innermost block. The caller has checked that its name is free.
    pub fn declare(&mut self, local: Local<'src>) -> LocalId {
        let id = self.locals.len();
        self.visible.insert(local.name.text, id);
        if let Some(block) = self.blocks.last_mut() {
            block.push(id);
        }
        self.locals.push(local);
        id
    }

    pub fn open_block(&mut self) {
        self.blocks.push(Vec::new());
    }

    /// Ends the innermost block and returns the locals that go out of scope with it.
    pub fn close_block(&mut self) -> Vec<LocalId> {
        let closed = self.blocks.pop().unwrap_or_default();
        for id in &closed {
            if let Some(local) = self.locals.get(*id) {
                self.visible.remove(local.name.text);
            }
        }
        closed
    }

    pub fn enter_loop(&mut self) {
        self.loops.push(Loop::default());
    }

    /// Ends the innermost loop and says whether a `break` leaves it.
    pub fn exit_loop(&mut self) -> bool {
        self.loops
            .pop()
            .is_some_and(|innermost| innermost.has_break)
    }

    pub fn in_loop(&self) -> bool {
        !self.loops.is_empty()
    }

    pub fn record_break(&mut self) {
        if let Some(innermost) = self.loops.last_mut() {
            innermost.has_break = true;
        }
    }

    /// The locals for the checked program, or `Err` if any declaration was rejected.
    pub fn into_locals(self) -> Result<Vec<ir::Local<'src>>, ErrorReported> {
        self.locals
            .into_iter()
            .map(|local| {
                Ok(ir::Local {
                    name: local.name.text,
                    ty: local.ty?,
                    mutable: local.kind.is_mutable(),
                    by_reference: local.kind == LocalKind::VariableParameter,
                })
            })
            .collect()
    }
}
