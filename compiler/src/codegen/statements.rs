/*!
 * C for statements. An expression may need statements of its own first; they
 * go right before the statement that uses it.
 *
 * Each C block frees the lists it built when control leaves it: at its end,
 * and at every `return`, `break` and `continue` that leaves it early. A list a
 * statement computes without a binding is freed once the statement is done.
 */

use super::expressions::{Bounds, is_addressable};
use super::types::list_function;
use super::{
    DESTINATION, Generator, INDENT, Release, Scope, Writer, c_syntax, data_stack, local_access,
    local_address,
};
use crate::ir::{
    Block, Branch, Case, Expr, Extent, FloatType, Function, LocalId, Place, PlaceStep, PrintPart,
    Statement, TextForm,
};

impl Generator<'_, '_> {
    pub(super) fn block_contents(
        &mut self,
        function: &Function<'_>,
        block: &Block,
        writer: &mut Writer,
    ) {
        self.scoped_block(function, block, false, writer);
    }

    /// A block that frees the lists it builds when control reaches its end. A loop's body is the block `break` and `continue` leave.
    fn scoped_block(
        &mut self,
        function: &Function<'_>,
        block: &Block,
        loop_body: bool,
        writer: &mut Writer,
    ) {
        self.scopes.push(Scope {
            releases: Vec::new(),
            loop_body,
        });
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

    /// The body of a loop, one level in.
    fn loop_body(&mut self, function: &Function<'_>, body: &Block, writer: &mut Writer) {
        writer.depth += 1;
        self.scoped_block(function, body, true, writer);
        writer.depth -= 1;
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
                if target.steps.is_empty() && value.ty.is_growable() =>
            {
                self.replace_list(function, target.local, value, writer);
            }
            Statement::Assign { target, value } => self.assign(function, target, value, writer),
            Statement::Call(call) => {
                let call = self.call(function, call, None, writer);
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

    /// A declaration. A list binding gets a data stack of its own, which the block frees when control leaves it.
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
        if !declared.ty.is_growable() {
            let value = self.expr(function, value, writer);
            writer.line(&format!("{constness}{ty} {name} = {value};"));
            return;
        }
        let stack = self.new_data_stack();
        let code = self.build_list(function, value, &data_stack(stack), writer);
        writer.line(&format!("{constness}{ty} {name} = {code};"));
        if let Some(scope) = self.scopes.last_mut() {
            scope.releases.push(Release { stack, list: name });
        }
    }

    /**
     * `items = value` for a list. Only the newest value on a data stack can be
     * replaced, so the runtime checks that first. A value that reads the old
     * list, itself or through a view, is built right above it and then moved
     * down into its place; any other replaces it after it is freed, with no
     * copy (spec 7.9).
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
        let stack = format!("({list}).stack");
        let reads_a_view = (0..function.locals.len())
            .any(|local| function.locals[local].ty.as_view().is_some() && value.mentions(local));
        if value.mentions(target) || reads_a_view {
            let code = self.build_list(function, value, &stack, writer);
            let built = self.temporary(&value.ty, &code, writer);
            let list_type = self.types.name(&value.ty);
            writer.line(&format!(
                "{}(&({list}), {built});",
                list_function(&list_type, "replace")
            ));
        } else {
            writer.line(&format!(
                "lymra_data_stack_release({stack}, (char *)({list}).elements);"
            ));
            let code = self.build_list(function, value, &stack, writer);
            writer.line(&format!("{list} = {code};"));
        }
    }

    /**
     * A `return`. Every list the function has open is freed first. A returned
     * list is then moved to the top of the stack the caller named: freeing
     * leaves the bytes in place, and the move copies them like `memmove`, so
     * it works even where the result lay on that same stack.
     */
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
        let code = self.expr(function, value, writer);
        let open = self.open_lists();
        if !value.ty.is_growable() {
            if open.is_empty() {
                writer.line(&format!("return {code};"));
            } else {
                let result = self.temporary(&value.ty, &code, writer);
                Self::release(writer, &open);
                writer.line(&format!("return {result};"));
            }
            self.pending.clear();
            return;
        }
        let ty = self.types.name(&value.ty);
        let result = self.helper("result");
        writer.line(&format!("const {ty} {result} = {code};"));
        // Dropped first and settled after the move, so AddressSanitizer lets the move read what was freed.
        for release in open.iter().rev() {
            writer.line(&format!(
                "lymra_data_stack_drop({}, {});",
                data_stack(release.stack),
                release.start()
            ));
        }
        let site = self.site(value.span);
        let moved = self.helper("moved");
        writer.line(&format!(
            "const {ty} {moved} = {}({DESTINATION}, {result}.elements, {result}.length, {site});",
            list_function(&ty, "place")
        ));
        for release in &open {
            writer.line(&format!(
                "lymra_data_stack_settle({});",
                data_stack(release.stack)
            ));
        }
        self.pending.clear();
        writer.line(&format!("return {moved};"));
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
        // A field has no effects; an index has its check and those of its value.
        let effects: Vec<bool> = target
            .steps
            .iter()
            .map(|step| match step {
                PlaceStep::Index(index) => index.checked || index.value.has_effects(),
                PlaceStep::Field(_) => false,
            })
            .chain([value.has_effects()])
            .collect();
        let mut place = local_access(function, target.local);
        for (position, step) in target.steps.iter().enumerate() {
            let index = match step {
                PlaceStep::Index(index) => index,
                PlaceStep::Field(field) => {
                    place = format!("{place}.{}", c_syntax::field_name(*field));
                    continue;
                }
            };
            let bounds = Bounds {
                length: index.length,
                checked: index.checked,
                span: index.span,
                // Only arrays and lists can be changed through an index; a view only reads.
                indexed: match index.length {
                    Extent::Fixed(_) => "an array",
                    Extent::Dynamic => "a list",
                },
            };
            let code = self.position(function, &index.value, bounds, &place, writer);
            let later_effect = effects[position + 1..].iter().any(|&effect| effect);
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
        writer.line(&format!("{place} = {code};"));
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
        self.loop_body(function, body, writer);
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
        self.loop_body(function, body, writer);
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
        writer.depth += 1;
        if let Some(variable) = variable {
            let local = &function.locals[variable];
            let element = self.types.name(&local.ty);
            let name = c_syntax::local_name(local.name);
            writer.line(&format!(
                "const {element} {name} = {items}->elements[{position}];"
            ));
        }
        writer.depth -= 1;
        self.loop_body(function, body, writer);
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
            for binding in &case.bindings {
                let local = &function.locals[binding.local];
                writer.line(&format!(
                    "const {} {} = {matched}->{}.{}.{};",
                    self.types.name(&local.ty),
                    c_syntax::local_name(local.name),
                    c_syntax::VARIANTS,
                    c_syntax::variant_name(case.variant),
                    c_syntax::field_name(binding.field)
                ));
            }
            self.block_contents(function, &case.body, writer);
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
