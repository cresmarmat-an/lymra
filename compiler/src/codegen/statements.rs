//! C for statements. An expression may need statements of its own first; they go right before the statement that uses it.

use super::expressions::{Bounds, is_addressable};
use super::{Generator, INDENT, Writer, c_syntax, local_access};
use crate::ir::{
    Block, Branch, Expr, Function, LocalId, Place, PrintPart, Statement, TextForm, Type,
};

impl Generator<'_, '_> {
    pub(super) fn block_contents(
        &mut self,
        function: &Function<'_>,
        block: &Block,
        writer: &mut Writer,
    ) {
        for statement in &block.statements {
            self.statement(function, statement, writer);
        }
    }

    fn nested_block(&mut self, function: &Function<'_>, block: &Block, writer: &mut Writer) {
        writer.depth += 1;
        self.block_contents(function, block, writer);
        writer.depth -= 1;
    }

    fn statement(&mut self, function: &Function<'_>, statement: &Statement, writer: &mut Writer) {
        match statement {
            Statement::Declare { local, value } => {
                let declared = &function.locals[*local];
                let constness = if declared.mutable { "" } else { "const " };
                let value = self.expr(function, value, writer);
                writer.line(&format!(
                    "{constness}{} {} = {value};",
                    self.types.name(&declared.ty),
                    c_syntax::local_name(declared.name)
                ));
            }
            Statement::Discard(value) => {
                let value = self.expr(function, value, writer);
                writer.line(&format!("(void)({value});"));
            }
            Statement::Assign { target, value } => self.assign(function, target, value, writer),
            Statement::Call(call) => {
                let call = self.call(function, call, writer);
                writer.line(&format!("{call};"));
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
            Statement::Return(Some(value)) => {
                let value = self.expr(function, value, writer);
                writer.line(&format!("return {value};"));
            }
            Statement::Return(None) => writer.line("return;"),
            Statement::Break => writer.line("break;"),
            Statement::Continue => writer.line("continue;"),
            Statement::Assert { condition } => {
                let site = self.site(condition.span);
                let condition = self.condition(function, condition, writer);
                writer.line(&format!("if (!({condition})) {{"));
                writer.line(&format!(
                    "{INDENT}lymra_trap(LYMRA_ASSERTION_FAILED, {site});"
                ));
                writer.line("}");
            }
        }
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
        let effects: Vec<bool> = target
            .indexes
            .iter()
            .map(|index| index.checked || index.value.has_effects())
            .chain([value.has_effects()])
            .collect();
        let mut place = local_access(function, target.local);
        for (position, index) in target.indexes.iter().enumerate() {
            let bounds = Bounds {
                length: index.length,
                checked: index.checked,
                span: index.span,
            };
            let code = self.position(function, &index.value, bounds, writer);
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
         * A C compiler may build a returned array right where it is assigned.
         * Where the call can still read that storage, as in `board =
         * step(board)`, it is built in a temporary first (spec section 5).
         */
        let code = if matches!(value.ty, Type::Array(_)) && value.mentions(target.local) {
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
                let condition = self.condition(function, &branch.condition, writer);
                writer.line(&format!("if ({condition}) {{"));
            } else {
                let mut setup = Writer::at(writer.depth + 1);
                let condition = self.condition(function, &branch.condition, &mut setup);
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
        let condition = self.condition(function, condition, &mut setup);
        if setup.is_empty() {
            writer.line(&format!("while ({condition}) {{"));
        } else {
            writer.line("for (;;) {");
            writer.append(setup);
            writer.line(&format!("{INDENT}if (!({condition})) {{"));
            writer.line(&format!("{INDENT}{INDENT}break;"));
            writer.line(&format!("{INDENT}}}"));
        }
        self.nested_block(function, body, writer);
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
        writer.line("{");
        writer.depth += 1;
        let start = self.expr(function, start, writer);
        writer.line(&format!("{ty} {counter} = {start};"));
        let end = self.expr(function, end, writer);
        writer.line(&format!("const {ty} {limit} = {end};"));
        writer.line(&format!("for (; {counter} < {limit}; ++{counter}) {{"));
        writer.depth += 1;
        if let Some(variable) = variable {
            let name = c_syntax::local_name(function.locals[variable].name);
            writer.line(&format!("const {ty} {name} = {counter};"));
        }
        self.block_contents(function, body, writer);
        writer.depth -= 1;
        writer.line("}");
        writer.depth -= 1;
        writer.line("}");
    }

    /**
     * `for variable in array`. The array is computed once, and a pointer
     * keeps where it is; each element is read when its turn comes.
     */
    fn for_each(
        &mut self,
        function: &Function<'_>,
        variable: Option<LocalId>,
        array: &Expr,
        length: u64,
        body: &Block,
        writer: &mut Writer,
    ) {
        let array_name = self.types.name(&array.ty);
        let items = self.helper("items");
        let position = self.helper("position");
        writer.line("{");
        writer.depth += 1;
        let code = self.expr(function, array, writer);
        let address = if is_addressable(array) {
            format!("&({code})")
        } else {
            format!("&{}", self.temporary(&array.ty, &code, writer))
        };
        writer.line(&format!("const {array_name} *{items} = {address};"));
        writer.line(&format!(
            "for (size_t {position} = 0; {position} < {length}u; ++{position}) {{"
        ));
        writer.depth += 1;
        if let Some(variable) = variable {
            let local = &function.locals[variable];
            let element = self.types.name(&local.ty);
            let name = c_syntax::local_name(local.name);
            writer.line(&format!(
                "const {element} {name} = {items}->elements[{position}];"
            ));
        }
        self.block_contents(function, body, writer);
        writer.depth -= 1;
        writer.line("}");
        writer.depth -= 1;
        writer.line("}");
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
    }
}
