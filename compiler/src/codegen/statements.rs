/*!
 * C for statements. An expression may need statements of its own first; they
 * go right before the statement that uses it.
 *
 * Each C block frees the lists it built when control leaves it: at its end,
 * and at every `return`, `break` and `continue` that leaves it early. A list a
 * statement computes without a binding is freed once the statement is done.
 */

use super::expressions::{Bounds, indexed_noun, is_addressable};
use super::text_types::{TextWalk, text_function};
use super::types::list_function;
use super::{
    DESTINATION, Generator, INDENT, Region, Release, Scope, Storage, Writer, c_syntax, data_stack,
    destination_names, local_access, local_address, releases_of,
};
use crate::ir::{
    Block, Branch, Case, Expr, ExprKind, Extent, FloatType, Function, LocalId, Place, PlaceStep,
    PrintPart, Statement, TextForm, Type,
};

impl Generator<'_, '_> {
    pub(super) fn block_contents(
        &mut self,
        function: &Function<'_>,
        block: &Block,
        writer: &mut Writer,
    ) {
        self.scoped_block(function, block, false, &[], writer);
    }

    /**
     * A block that frees the lists and text it builds when control reaches
     * its end. A loop's body is the block `break` and `continue` leave. The
     * block first declares `bindings`, as a `for` loop's variable or a
     * case's bindings.
     */
    fn scoped_block(
        &mut self,
        function: &Function<'_>,
        block: &Block,
        loop_body: bool,
        bindings: &[Binding],
        writer: &mut Writer,
    ) {
        self.scopes.push(Scope {
            releases: Vec::new(),
            loop_body,
        });
        for binding in bindings {
            self.bind(function, binding, writer);
        }
        for statement in &block.statements {
            self.statement(function, statement, writer);
        }
        let scope = self.scopes.pop().unwrap_or_default();
        let leaves = matches!(
            block.statements.last(),
            Some(Statement::Return(_) | Statement::Break | Statement::Continue)
        );
        if !leaves {
            Self::release(writer, &scope.releases);
        }
    }

    fn nested_block(&mut self, function: &Function<'_>, block: &Block, writer: &mut Writer) {
        writer.depth += 1;
        self.block_contents(function, block, writer);
        writer.depth -= 1;
    }

    /// The body of a loop, one level in, which first declares `bindings` as `scoped_block` does.
    fn loop_body(
        &mut self,
        function: &Function<'_>,
        body: &Block,
        bindings: &[Binding],
        writer: &mut Writer,
    ) {
        writer.depth += 1;
        self.scoped_block(function, body, true, bindings, writer);
        writer.depth -= 1;
    }

    /**
     * Declares a binding at the start of a block. One that copies its text
     * keeps it on a data stack of its own, which the block frees, so
     * changing what it was read from inside the block cannot change it.
     */
    fn bind(&mut self, function: &Function<'_>, binding: &Binding, writer: &mut Writer) {
        let declared = &function.locals[binding.local];
        let ty = self.types.name(&declared.ty);
        let name = c_syntax::local_name(declared.name);
        let source = &binding.source;
        if !binding.copies_text || Storage::of(&declared.ty) != Storage::Text {
            writer.line(&format!("const {ty} {name} = {source};"));
            return;
        }
        let (stacks, start) = self.new_stacks(Storage::Text, writer);
        let own = text_function(&mut self.types, &declared.ty, TextWalk::Own);
        let site = self.site(declared.span);
        writer.line(&format!("{ty} {name} = {source};"));
        writer.line(&format!(
            "{own}(&{name}, {}, {site});",
            data_stack(stacks[0])
        ));
        let releases = releases_of(Storage::Text, &stacks, &name, start);
        if let Some(scope) = self.scopes.last_mut() {
            scope.releases.extend(releases);
        }
    }

    /// Opens a C block that a statement keeps lists in across its own blocks, such as a loop over a list a call returns.
    fn open_wrapper(&mut self, writer: &mut Writer) {
        writer.line("{");
        writer.depth += 1;
        self.scopes.push(Scope::default());
    }

    /// Moves the lists the statement has computed so far into the wrapper, which frees them when it closes.
    fn keep_in_wrapper(&mut self) {
        let built = std::mem::take(&mut self.pending);
        if let Some(scope) = self.scopes.last_mut() {
            scope.releases.extend(built);
        }
    }

    fn close_wrapper(&mut self, writer: &mut Writer) {
        let scope = self.scopes.pop().unwrap_or_default();
        Self::release(writer, &scope.releases);
        writer.depth -= 1;
        writer.line("}");
    }

    fn statement(&mut self, function: &Function<'_>, statement: &Statement, writer: &mut Writer) {
        self.statement_kind(function, statement, writer);
        let built = std::mem::take(&mut self.pending);
        Self::release(writer, &built);
    }

    fn statement_kind(
        &mut self,
        function: &Function<'_>,
        statement: &Statement,
        writer: &mut Writer,
    ) {
        match statement {
            Statement::Declare { local, value } => self.declare(function, *local, value, writer),
            Statement::Discard(value) => {
                let value = self.expr(function, value, writer);
                writer.line(&format!("(void)({value});"));
            }
            Statement::Assign { target, value }
                if target.steps.is_empty() && Storage::of(&value.ty) == Storage::Text =>
            {
                self.replace_text(function, target.local, value, writer);
            }
            Statement::Assign { target, value }
                if target.steps.is_empty() && Storage::of(&value.ty) != Storage::None =>
            {
                self.replace_list(function, target.local, value, writer);
            }
            Statement::Assign { target, value } => self.assign(function, target, value, writer),
            Statement::Call(call) => {
                let call = self.call(function, call, &[], writer);
                writer.line(&format!("{call};"));
            }
            Statement::Push { list, value, span } => {
                let code = self.expr(function, value, writer);
                let site = self.site(*span);
                let list_type = self.types.name(&function.locals[*list].ty);
                writer.line(&format!(
                    "{}({}, {code}, {site});",
                    list_function(&list_type, "push"),
                    local_address(function, *list)
                ));
            }
            Statement::Remove { pool, handle, span } => {
                let handle = self.expr(function, handle, writer);
                let site = self.site(*span);
                let pool_type = self.types.name(&function.locals[*pool].ty);
                writer.line(&format!(
                    "{}({}, {handle}, {site});",
                    list_function(&pool_type, "remove"),
                    local_address(function, *pool)
                ));
            }
            Statement::Print(parts) => self.print(function, parts, writer),
            Statement::If {
                branches,
                otherwise,
            } => self.if_chain(function, branches, otherwise.as_ref(), writer),
            Statement::While { condition, body } => {
                self.while_loop(function, condition, body, writer);
            }
            Statement::ForRange {
                variable,
                start,
                end,
                body,
            } => self.for_range(function, *variable, start, end, body, writer),
            Statement::ForEach {
                variable,
                array,
                length,
                body,
            } => self.for_each(function, *variable, array, *length, body, writer),
            Statement::Match { value, cases } => self.match_cases(function, value, cases, writer),
            Statement::Return(value) => self.return_statement(function, value.as_ref(), writer),
            Statement::Break => {
                Self::release(writer, &self.loop_lists());
                writer.line("break;");
            }
            Statement::Continue => {
                Self::release(writer, &self.loop_lists());
                writer.line("continue;");
            }
            Statement::Assert { condition } => {
                let site = self.site(condition.span);
                let condition = self.condition_now(function, condition, writer);
                writer.line(&format!("if (!({condition})) {{"));
                writer.line(&format!(
                    "{INDENT}lymra_trap(LYMRA_ASSERTION_FAILED, {site});"
                ));
                writer.line("}");
            }
        }
    }

    /**
     * A declaration. A binding that keeps a list or text gets data stacks of
     * its own, which the block frees when control leaves it. A constant that
     * holds only text the program itself keeps needs none.
     */
    fn declare(
        &mut self,
        function: &Function<'_>,
        local: LocalId,
        value: &Expr,
        writer: &mut Writer,
    ) {
        let declared = &function.locals[local];
        let constness = if declared.mutable { "" } else { "const " };
        let ty = self.types.name(&declared.ty);
        let name = c_syntax::local_name(declared.name);
        let storage = Storage::of(&declared.ty);
        let keeps_nothing = storage == Storage::None
            || (storage == Storage::Text && !declared.mutable && is_static_text(value));
        if keeps_nothing {
            let value = self.expr(function, value, writer);
            writer.line(&format!("{constness}{ty} {name} = {value};"));
            return;
        }
        let (stacks, text_start) = self.new_stacks(storage, writer);
        let names: Vec<String> = stacks.iter().map(|&stack| data_stack(stack)).collect();
        let code = self.build_value(function, value, &names, writer);
        writer.line(&format!("{constness}{ty} {name} = {code};"));
        if let (Some(stack), Some(start)) = (names.first(), &text_start) {
            self.text_regions.insert(
                local,
                Region {
                    stack: stack.clone(),
                    start: start.clone(),
                },
            );
        }
        let releases = releases_of(storage, &stacks, &name, text_start);
        if let Some(scope) = self.scopes.last_mut() {
            scope.releases.extend(releases);
        }
    }

    /**
     * `items = value` for a list. Only the newest value on a data stack can be
     * replaced, so the runtime checks that first. A value that reads the old
     * list, itself or through a view, is built right above it and then moved
     * down into its place, its text too; any other replaces it after it is
     * freed, with no copy (spec 7.9).
     */
    fn replace_list(
        &mut self,
        function: &Function<'_>,
        target: LocalId,
        value: &Expr,
        writer: &mut Writer,
    ) {
        let list = local_access(function, target);
        let site = self.site(value.span);
        writer.line(&format!(
            "lymra_data_stack_require_top(({list}).stack, (const char *)(({list}).elements + ({list}).length), {site});"
        ));
        let mut stacks = vec![format!("({list}).stack")];
        let holds_text = Storage::of(&value.ty) == Storage::ElementsAndText;
        if holds_text {
            stacks.push(format!("({list}).text_stack"));
        }
        if reads_old_value(function, target, value) {
            let code = self.build_value(function, value, &stacks, writer);
            let built = self.temporary(&value.ty, &code, writer);
            let list_type = self.types.name(&value.ty);
            writer.line(&format!(
                "{}(&({list}), {built});",
                list_function(&list_type, "replace")
            ));
        } else {
            writer.line(&format!(
                "lymra_data_stack_release({}, (char *)({list}).elements);",
                stacks[0]
            ));
            if holds_text {
                writer.line(&format!(
                    "lymra_data_stack_release({}, ({list}).text);",
                    stacks[1]
                ));
            }
            let code = self.build_value(function, value, &stacks, writer);
            writer.line(&format!("{list} = {code};"));
        }
    }

    /**
     * `text = value` for a binding that keeps text. A value that reads the
     * old text is built right above it and then moved down into its place;
     * any other replaces it after it is freed, with no copy.
     */
    fn replace_text(
        &mut self,
        function: &Function<'_>,
        target: LocalId,
        value: &Expr,
        writer: &mut Writer,
    ) {
        let place = local_access(function, target);
        let Some(region) = self.text_regions.get(&target).cloned() else {
            let code = self.expr(function, value, writer);
            writer.line(&format!("{place} = {code};"));
            return;
        };
        let site = self.site(value.span);
        self.require_text_on_top(&function.locals[target].ty, &place, &region, &site, writer);
        let stacks = [region.stack.clone()];
        if reads_old_value(function, target, value) {
            let above = self.helper("above");
            writer.line(&format!("char *const {above} = ({})->top;", region.stack));
            let code = self.build_value(function, value, &stacks, writer);
            let ty = self.types.name(&value.ty);
            let built = self.helper("built");
            writer.line(&format!("{ty} {built} = {code};"));
            self.lower_text(&value.ty, &built, &region, &above, writer);
            writer.line(&format!("{place} = {built};"));
        } else {
            writer.line(&format!(
                "lymra_data_stack_release({}, {});",
                region.stack, region.start
            ));
            let code = self.build_value(function, value, &stacks, writer);
            writer.line(&format!("{place} = {code};"));
        }
    }

    /**
     * Moves the text of `value`, of type `ty`, built on the stack of `region`
     * from `above` up, down to where `region` starts, and points `value` at
     * it there.
     */
    fn lower_text(
        &mut self,
        ty: &Type,
        value: &str,
        region: &Region,
        above: &str,
        writer: &mut Writer,
    ) {
        let end = self.helper("end");
        writer.line(&format!("char *const {end} = ({})->top;", region.stack));
        let distance = self.helper("distance");
        writer.line(&format!(
            "const size_t {distance} = lymra_data_stack_lower({}, {}, {above});",
            region.stack, region.start
        ));
        let rebase = text_function(&mut self.types, ty, TextWalk::Rebase);
        writer.line(&format!("{rebase}(&{value}, {above}, {end}, {distance});"));
    }

    /**
     * Gathers the text of the binding `local` at the start of its region
     * again, after a part of it got new text: every piece is copied above
     * what the region holds, in order, then moved down into place, so the
     * text the binding no longer holds is freed.
     */
    fn compact_text(
        &mut self,
        function: &Function<'_>,
        local: LocalId,
        region: &Region,
        site: &str,
        writer: &mut Writer,
    ) {
        let place = local_access(function, local);
        let ty = &function.locals[local].ty;
        let above = self.helper("above");
        writer.line(&format!("char *const {above} = ({})->top;", region.stack));
        let own = text_function(&mut self.types, ty, TextWalk::Own);
        writer.line(&format!("{own}(&{place}, {}, {site});", region.stack));
        self.lower_text(ty, &place, region, &above, writer);
    }

    /**
     * Stops the program unless the text of `place`, of type `ty`, which
     * lies in `region`, is the newest on its data stack: new text for it
     * goes where its text starts, over anything newer. The compiler proved
     * it (L0704); the runtime checks it again, as it does for a `push`.
     */
    fn require_text_on_top(
        &mut self,
        ty: &Type,
        place: &str,
        region: &Region,
        site: &str,
        writer: &mut Writer,
    ) {
        let highest = text_function(&mut self.types, ty, TextWalk::Highest);
        let Region { stack, start } = region;
        writer.line(&format!(
            "lymra_data_stack_require_top({stack}, {highest}(&{place}, {stack}, {start}), {site});"
        ));
    }

    /// A `return`. Every list and text the function has open is freed first.
    fn return_statement(
        &mut self,
        function: &Function<'_>,
        value: Option<&Expr>,
        writer: &mut Writer,
    ) {
        let Some(value) = value else {
            Self::release(writer, &self.open_lists());
            writer.line("return;");
            return;
        };
        match Storage::of(&value.ty) {
            Storage::None => self.return_fixed(function, value, writer),
            // Text the program itself keeps outlives every call, so it goes back as it is.
            Storage::Text if is_static_text(value) => self.return_fixed(function, value, writer),
            Storage::Text => self.return_text(function, value, writer),
            storage @ (Storage::Elements | Storage::ElementsAndText) => {
                self.return_list(function, value, storage, writer);
            }
        }
        self.pending.clear();
    }

    /// Returns `value`, which keeps nothing on a data stack, or only text the program itself keeps.
    fn return_fixed(&mut self, function: &Function<'_>, value: &Expr, writer: &mut Writer) {
        let code = self.expr(function, value, writer);
        let open = self.open_lists();
        if open.is_empty() {
            writer.line(&format!("return {code};"));
            return;
        }
        let result = self.temporary(&value.ty, &code, writer);
        Self::release(writer, &open);
        writer.line(&format!("return {result};"));
    }

    /**
     * Returns `value`, a list or a stack, moved to the top of the stacks the
     * caller named. Freeing leaves the bytes in place, and the move copies
     * them like `memmove`, so it works even where the result lay on those
     * same stacks.
     */
    fn return_list(
        &mut self,
        function: &Function<'_>,
        value: &Expr,
        storage: Storage,
        writer: &mut Writer,
    ) {
        let code = self.expr(function, value, writer);
        let open = self.open_lists();
        let ty = self.types.name(&value.ty);
        let result = self.helper("result");
        writer.line(&format!("const {ty} {result} = {code};"));
        // Dropped first and settled after the move, so AddressSanitizer lets the move read what was freed.
        for release in open.iter().rev() {
            writer.line(&format!(
                "lymra_data_stack_drop({}, {});",
                data_stack(release.stack),
                release.start
            ));
        }
        let site = self.site(value.span);
        let moved = self.helper("moved");
        writer.line(&format!(
            "const {ty} {moved} = {}({}, {result}, {site});",
            list_function(&ty, "place"),
            destination_names(storage).join(", ")
        ));
        for release in &open {
            writer.line(&format!(
                "lymra_data_stack_settle({});",
                data_stack(release.stack)
            ));
        }
        writer.line(&format!("return {moved};"));
    }

    /**
     * Returns `value`, which holds text. Its pieces can lie anywhere, even in
     * what the function frees, so they go onto a stack the function keeps
     * for that before anything is freed, and from there onto the caller's.
     * Text built in the `return` itself is built right on that stack, and
     * text that nothing frees goes straight to the caller.
     */
    fn return_text(&mut self, function: &Function<'_>, value: &Expr, writer: &mut Writer) {
        let ty = self.types.name(&value.ty);
        let own = text_function(&mut self.types, &value.ty, TextWalk::Own);
        let site = self.site(value.span);
        let result = self.helper("result");
        let built_here = match &value.kind {
            ExprKind::Text(parts) => Some(parts),
            _ => None,
        };
        let kept = if let Some(parts) = built_here {
            let (stack, keep) = self.keep_return_stack(writer);
            let code = self.build_text(function, parts, &stack, value.span, writer);
            writer.line(&format!("{ty} {result} = {code};"));
            Some((stack, keep))
        } else {
            let code = self.expr(function, value, writer);
            writer.line(&format!("{ty} {result} = {code};"));
            if self.open_lists().is_empty() {
                None
            } else {
                let (stack, keep) = self.keep_return_stack(writer);
                writer.line(&format!("{own}(&{result}, {stack}, {site});"));
                Some((stack, keep))
            }
        };
        Self::release(writer, &self.open_lists());
        writer.line(&format!("{own}(&{result}, {DESTINATION}, {site});"));
        if let Some((stack, keep)) = kept {
            writer.line(&format!("lymra_data_stack_release({stack}, {keep});"));
        }
        writer.line(&format!("return {result};"));
    }

    /**
     * The stack this function keeps for the text it returns, and a note of
     * its top, which the `return` moves it back to once the caller has the
     * text. No other value lives on that stack, and every `return` leaves it
     * as it found it, so a recursive call can use it too.
     */
    fn keep_return_stack(&mut self, writer: &mut Writer) -> (String, String) {
        let stack = match self.return_stack {
            Some(stack) => stack,
            None => {
                let stack = self.new_data_stack();
                self.return_stack = Some(stack);
                stack
            }
        };
        let stack = data_stack(stack);
        let keep = self.helper("keep");
        writer.line(&format!("char *const {keep} = ({stack})->top;"));
        (stack, keep)
    }

    /**
     * A condition of an `if`, a `while` or an `assert`. Lists computed for it
     * are freed as soon as it is known, which it then holds as a `bool`.
     */
    fn condition_now(
        &mut self,
        function: &Function<'_>,
        condition: &Expr,
        writer: &mut Writer,
    ) -> String {
        let earlier = self.pending.len();
        let code = self.condition(function, condition, writer);
        if self.pending.len() == earlier {
            return code;
        }
        let test = self.helper("test");
        writer.line(&format!("const bool {test} = {code};"));
        let built: Vec<Release> = self.pending.drain(earlier..).collect();
        Self::release(writer, &built);
        test
    }

    /**
     * `target = value`. The target's indexes are computed and checked from the
     * binding inward, then the value; one whose effects could show before a
     * later part's is computed into a temporary first.
     */
    fn assign(
        &mut self,
        function: &Function<'_>,
        target: &Place,
        value: &Expr,
        writer: &mut Writer,
    ) {
        // A field has no effects; an index has its check and those of its value, and a slot always has its check.
        let effects: Vec<bool> = target
            .steps
            .iter()
            .map(|step| match step {
                PlaceStep::Index(index) => index.checked || index.value.has_effects(),
                PlaceStep::Slot(_) => true,
                PlaceStep::Field(_) => false,
            })
            .chain([value.has_effects()])
            .collect();
        let mut place = local_access(function, target.local);
        for (position, step) in target.steps.iter().enumerate() {
            let later_effect = effects[position + 1..].iter().any(|&effect| effect);
            let index = match step {
                PlaceStep::Index(index) => index,
                PlaceStep::Field(field) => {
                    place = format!("{place}.{}", c_syntax::field_name(*field));
                    continue;
                }
                PlaceStep::Slot(slot) => {
                    let handle = self.expr(function, &slot.handle, writer);
                    let found = self.pool_slot(&slot.pool, &place, &handle, slot.span);
                    // Found first, so a stale handle stops the program before anything after it runs.
                    let found = if later_effect {
                        let name = self.helper("slot");
                        let slot_type = format!("{}_slot", self.types.name(&slot.pool));
                        writer.line(&format!("{slot_type} *const {name} = {found};"));
                        name
                    } else {
                        found
                    };
                    place = format!("{found}->value");
                    continue;
                }
            };
            let bounds = Bounds {
                length: index.length(),
                checked: index.checked,
                span: index.span,
                indexed: indexed_noun(&index.indexed),
            };
            let code = self.position(function, &index.value, bounds, &place, writer);
            let code = if effects[position] && later_effect {
                let name = self.helper("position");
                writer.line(&format!("const size_t {name} = {code};"));
                name
            } else {
                code
            };
            place = format!("{place}.elements[{code}]");
        }
        let code = self.expr(function, value, writer);
        /*
         * A C compiler may build a returned value right where it is assigned.
         * Where the call can still read that storage, as in `board =
         * step(board)`, it is built in a temporary first (spec section 5).
         */
        let code = if value.ty.is_compound() && value.mentions(target.local) {
            self.temporary(&value.ty, &code, writer)
        } else {
            code
        };
        // New text in a part of a binding that keeps text: the binding gathers its text again.
        let region = if value.ty.holds_text() {
            self.text_regions.get(&target.local).cloned()
        } else {
            None
        };
        let gathering = match region {
            Some(region) => {
                let site = self.site(value.span);
                let binding = local_access(function, target.local);
                let ty = &function.locals[target.local].ty;
                self.require_text_on_top(ty, &binding, &region, &site, writer);
                Some((region, site))
            }
            None => None,
        };
        writer.line(&format!("{place} = {code};"));
        if let Some((region, site)) = gathering {
            self.compact_text(function, target.local, &region, &site, writer);
        }
    }

    /**
     * An `if` chain. The statements a condition needs may only run once the
     * conditions before it failed, so from a condition that needs any, the
     * rest of the chain moves into an `else` block that runs them first.
     */
    fn if_chain(
        &mut self,
        function: &Function<'_>,
        branches: &[Branch],
        otherwise: Option<&Block>,
        writer: &mut Writer,
    ) {
        let mut opened = 0;
        for (index, branch) in branches.iter().enumerate() {
            if index == 0 {
                let condition = self.condition_now(function, &branch.condition, writer);
                writer.line(&format!("if ({condition}) {{"));
            } else {
                let mut setup = Writer::at(writer.depth + 1);
                let condition = self.condition_now(function, &branch.condition, &mut setup);
                if setup.is_empty() {
                    writer.line(&format!("}} else if ({condition}) {{"));
                } else {
                    writer.line("} else {");
                    writer.depth += 1;
                    writer.append(setup);
                    writer.line(&format!("if ({condition}) {{"));
                    opened += 1;
                }
            }
            self.nested_block(function, &branch.body, writer);
        }
        if let Some(otherwise) = otherwise {
            writer.line("} else {");
            self.nested_block(function, otherwise, writer);
        }
        writer.line("}");
        for _ in 0..opened {
            writer.depth -= 1;
            writer.line("}");
        }
    }

    /// A `while` loop. A condition that needs statements of its own runs them before every test.
    fn while_loop(
        &mut self,
        function: &Function<'_>,
        condition: &Expr,
        body: &Block,
        writer: &mut Writer,
    ) {
        let mut setup = Writer::at(writer.depth + 1);
        let condition = self.condition_now(function, condition, &mut setup);
        if setup.is_empty() {
            writer.line(&format!("while ({condition}) {{"));
        } else {
            writer.line("for (;;) {");
            writer.append(setup);
            writer.line(&format!("{INDENT}if (!({condition})) {{"));
            writer.line(&format!("{INDENT}{INDENT}break;"));
            writer.line(&format!("{INDENT}}}"));
        }
        self.loop_body(function, body, &[], writer);
        writer.line("}");
    }

    /**
     * `for variable in range(start, end)`: the start is computed, then the
     * end, each once, and the counter only counts up while it is below the
     * end, so it cannot overflow.
     */
    fn for_range(
        &mut self,
        function: &Function<'_>,
        variable: Option<LocalId>,
        start: &Expr,
        end: &Expr,
        body: &Block,
        writer: &mut Writer,
    ) {
        let ty = self.types.name(&start.ty);
        let counter = self.helper("range_next");
        let limit = self.helper("range_end");
        self.open_wrapper(writer);
        let start = self.expr(function, start, writer);
        writer.line(&format!("{ty} {counter} = {start};"));
        let end = self.expr(function, end, writer);
        writer.line(&format!("const {ty} {limit} = {end};"));
        self.keep_in_wrapper();
        writer.line(&format!("for (; {counter} < {limit}; ++{counter}) {{"));
        writer.depth += 1;
        if let Some(variable) = variable {
            let name = c_syntax::local_name(function.locals[variable].name);
            writer.line(&format!("const {ty} {name} = {counter};"));
        }
        writer.depth -= 1;
        self.loop_body(function, body, &[], writer);
        writer.line("}");
        self.close_wrapper(writer);
    }

    /**
     * `for variable in items`. The array or list is computed once, and a
     * pointer keeps where it is; each element is read when its turn comes. A
     * list's length is read once, before the first turn, so the body can add
     * to the list without lengthening the loop.
     */
    fn for_each(
        &mut self,
        function: &Function<'_>,
        variable: Option<LocalId>,
        array: &Expr,
        length: Extent,
        body: &Block,
        writer: &mut Writer,
    ) {
        let array_name = self.types.name(&array.ty);
        let items = self.helper("items");
        let position = self.helper("position");
        self.open_wrapper(writer);
        let code = self.expr(function, array, writer);
        // A view is copied: the loop goes over what it shows when the loop starts, whatever the binding shows later.
        let address = if is_addressable(array) && array.ty.as_view().is_none() {
            format!("&({code})")
        } else {
            format!("&{}", self.temporary(&array.ty, &code, writer))
        };
        self.keep_in_wrapper();
        writer.line(&format!("const {array_name} *{items} = {address};"));
        match length {
            Extent::Fixed(length) => writer.line(&format!(
                "for (size_t {position} = 0; {position} < {length}u; ++{position}) {{"
            )),
            Extent::Dynamic => {
                let count = self.helper("count");
                writer.line(&format!("const size_t {count} = (size_t){items}->length;"));
                writer.line(&format!(
                    "for (size_t {position} = 0; {position} < {count}; ++{position}) {{"
                ));
            }
        }
        // A list, a stack or a view keeps its elements' text in place while a loop goes over it (L0702, L0703, L0407).
        let copies_text = array.ty.as_array().is_some() && !keeps_text_in_place(function, array);
        let bindings: Vec<Binding> = variable
            .map(|local| Binding {
                local,
                source: format!("{items}->elements[{position}]"),
                copies_text,
            })
            .into_iter()
            .collect();
        self.loop_body(function, body, &bindings, writer);
        writer.line("}");
        self.close_wrapper(writer);
    }

    /**
     * `match value`. The value is computed once, and a pointer keeps where it
     * is; the tag picks the case, which copies the fields it names into its
     * bindings before its body runs. The cases form an `if` chain rather
     * than a `switch`, where C would take a `break` in a case as leaving the
     * `switch` instead of the loop around it. The cases handle every
     * variant, so the last one needs no test.
     */
    fn match_cases(
        &mut self,
        function: &Function<'_>,
        value: &Expr,
        cases: &[Case],
        writer: &mut Writer,
    ) {
        let choice = self.types.name(&value.ty);
        let matched = self.helper("matched");
        self.open_wrapper(writer);
        let code = self.expr(function, value, writer);
        // With one case and nothing to bind, the value is only computed.
        let reads_value = cases.len() > 1 || cases.iter().any(|case| !case.bindings.is_empty());
        if reads_value {
            let address = if is_addressable(value) {
                format!("&({code})")
            } else {
                format!("&{}", self.temporary(&value.ty, &code, writer))
            };
            writer.line(&format!("const {choice} *{matched} = {address};"));
        } else if value.has_effects() {
            writer.line(&format!("(void)({code});"));
        }
        self.keep_in_wrapper();
        let copies_case_text = !keeps_text_in_place(function, value);
        for (position, case) in cases.iter().enumerate() {
            let test = format!("{matched}->{} == {}", c_syntax::TAG, case.variant);
            let line = match (position, position + 1 == cases.len()) {
                (0, true) => "{".to_owned(),
                (0, false) => format!("if ({test}) {{"),
                (_, true) => "} else {".to_owned(),
                (_, false) => format!("}} else if ({test}) {{"),
            };
            writer.line(&line);
            writer.depth += 1;
            let bindings: Vec<Binding> = case
                .bindings
                .iter()
                .map(|binding| Binding {
                    local: binding.local,
                    source: format!(
                        "{matched}->{}.{}.{}",
                        c_syntax::VARIANTS,
                        c_syntax::variant_name(case.variant),
                        c_syntax::field_name(binding.field)
                    ),
                    copies_text: copies_case_text,
                })
                .collect();
            self.scoped_block(function, &case.body, false, &bindings, writer);
            writer.depth -= 1;
        }
        if !cases.is_empty() {
            writer.line("}");
        }
        self.close_wrapper(writer);
    }

    /**
     * A call to `print`. Every value is computed before the line starts, so a
     * value whose computation prints writes its own line first, and one that
     * stops the program leaves no part of this line behind.
     */
    fn print(&mut self, function: &Function<'_>, parts: &[PrintPart], writer: &mut Writer) {
        let writes: Vec<String> = parts
            .iter()
            .filter_map(|part| match part {
                PrintPart::Text(text) if text.is_empty() => None,
                PrintPart::Text(text) => {
                    Some(format!("lymra_write_text({})", c_syntax::text_value(text)))
                }
                PrintPart::Value { value, form } => {
                    self.writes_floats |= matches!(form, TextForm::Float(_));
                    let code = self.expr(function, value, writer);
                    let code = if value.has_effects() {
                        self.temporary(&value.ty, &code, writer)
                    } else {
                        code
                    };
                    Some(write_value(*form, &code))
                }
            })
            .collect();
        for write in writes {
            writer.line(&format!("{write};"));
        }
        writer.line("lymra_end_line();");
    }
}

/// The runtime call that writes `value` into the line being printed.
fn write_value(form: TextForm, value: &str) -> String {
    match form {
        TextForm::Text => format!("lymra_write_text({value})"),
        TextForm::Boolean => format!("lymra_write_boolean({value})"),
        TextForm::Signed => format!("lymra_write_signed((int64_t)({value}))"),
        TextForm::Unsigned => format!("lymra_write_unsigned((uint64_t)({value}))"),
        TextForm::Float(FloatType::Float32) => format!("lymra_write_float32({value})"),
        TextForm::Float(FloatType::Float64) => format!("lymra_write_float64({value})"),
    }
}

/// A local that a block declares before its statements: a `for` loop's variable or a case's binding.
struct Binding {
    local: LocalId,
    /// The C of its value.
    source: String,
    /// Whether it copies the text of its value, which the block could otherwise free or move.
    copies_text: bool,
}

/**
 * Whether the text in `value` stays where it is while a block that reads
 * it runs, so the block's bindings can read it in place. It does in a
 * constant, in a parameter, which no call can change through another
 * (L0508), in text written in the program, and in a value computed for
 * the block whose text was built for it: a call's result, or text built
 * from parts. Text that a `variable` binding keeps can be freed or moved by
 * the block, and so can text that a computed value reads from one, such as
 * `top(pending)` or `Said(words = name)`.
 */
fn keeps_text_in_place(function: &Function<'_>, value: &Expr) -> bool {
    match &value.kind {
        ExprKind::Local(_) | ExprKind::Index { .. } | ExprKind::Field { .. } => root_local(value)
            .is_some_and(|local| {
                let local = &function.locals[local];
                !local.mutable && !local.by_reference
            }),
        ExprKind::Literal(_) | ExprKind::Constant(_) | ExprKind::Call(_) | ExprKind::Text(_) => {
            true
        }
        ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => fields
            .iter()
            .all(|field| keeps_text_in_place(function, &field.value)),
        ExprKind::Fill(element) => keeps_text_in_place(function, element),
        _ => false,
    }
}

/// The binding that `value` is or is part of, read through fields and indexes.
fn root_local(value: &Expr) -> Option<LocalId> {
    match &value.kind {
        ExprKind::Local(local) => Some(*local),
        ExprKind::Index { array: object, .. } | ExprKind::Field { object, .. } => {
            root_local(object)
        }
        _ => None,
    }
}

/**
 * Whether `value` can read what `target` keeps before it is replaced: it
 * names `target`, or it reads a view, which can show it. Such a value is
 * built above the old one, which is freed only once the new one is done.
 */
fn reads_old_value(function: &Function<'_>, target: LocalId, value: &Expr) -> bool {
    value.mentions(target)
        || (0..function.locals.len())
            .any(|local| function.locals[local].ty.as_view().is_some() && value.mentions(local))
}

/// Whether `value` holds only text the program itself keeps, so a constant that holds it needs no data stack.
fn is_static_text(value: &Expr) -> bool {
    match &value.kind {
        ExprKind::Literal(_) | ExprKind::Constant(_) => true,
        ExprKind::Construct(fields) | ExprKind::Variant { fields, .. } => fields
            .iter()
            .all(|field| !field.value.ty.holds_text() || is_static_text(&field.value)),
        ExprKind::Fill(element) => !element.ty.holds_text() || is_static_text(element),
        _ => false,
    }
}
