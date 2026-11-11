/*!
 * Prints a syntax tree as an indented outline, for `lymra parse` and for tests.
 *
 * Declarations and statements get one line each, in Lymra's own word order.
 * Expressions are written in prefix form with full parentheses, so operator
 * precedence is visible: `1 + 2 * 3` prints as `(+ 1 (* 2 3))`.
 */

use super::{
    Argument, ArgumentKind, Binding, Block, BlockArgument, ChoiceDeclaration, Directive,
    DirectiveKind, Edition, Expr, ExprKind, Field, Function, IfStatement, Item, ItemKind, MatchArm,
    Module, Mutability, OptimizeMode, Parameter, Path, Statement, StatementKind, TextPart,
    TypeArgument, TypeDeclaration, TypeExpr, TypeExprKind, Visibility,
};

const INDENT: &str = "  ";

pub fn print_module(module: &Module<'_>) -> String {
    let mut printer = Printer {
        output: String::new(),
        depth: 0,
    };
    printer.line("module");
    printer.nested(|printer| {
        for directive in &module.directives {
            printer.line(&directive_text(directive));
        }
        for item in &module.items {
            printer.item(item);
        }
    });
    printer.output
}

struct Printer {
    output: String,
    depth: usize,
}

impl Printer {
    fn line(&mut self, text: &str) {
        self.output.push_str(&INDENT.repeat(self.depth));
        self.output.push_str(text);
        self.output.push('\n');
    }

    fn nested(&mut self, print: impl FnOnce(&mut Self)) {
        self.depth += 1;
        print(self);
        self.depth -= 1;
    }

    fn item(&mut self, item: &Item<'_>) {
        let visibility = match item.visibility {
            Visibility::Public => "public ",
            Visibility::Private => "",
        };
        match &item.kind {
            ItemKind::Use(path) => self.line(&format!("use {}", path_text(path))),
            ItemKind::Constant(binding) => self.binding(visibility, binding),
            ItemKind::Function(function) => self.function(visibility, function),
            ItemKind::Type(declaration) => self.type_declaration(visibility, declaration),
            ItemKind::Choice(declaration) => self.choice(visibility, declaration),
        }
    }

    fn function(&mut self, visibility: &str, function: &Function<'_>) {
        let parameters: Vec<String> = function.parameters.iter().map(parameter_text).collect();
        let returns = function
            .return_type
            .as_ref()
            .map(|ty| format!(" returns {}", type_text(ty)));
        let header = format!(
            "{visibility}function {}({}){}",
            function.name.text,
            parameters.join(", "),
            returns.unwrap_or_default()
        );
        self.line(&header);
        self.block(&function.body);
    }

    fn type_declaration(&mut self, visibility: &str, declaration: &TypeDeclaration<'_>) {
        self.line(&format!("{visibility}type {}", declaration.name.text));
        self.nested(|printer| {
            for field in &declaration.fields {
                printer.line(&format!("field {}", field_text(field)));
            }
        });
    }

    fn choice(&mut self, visibility: &str, declaration: &ChoiceDeclaration<'_>) {
        self.line(&format!("{visibility}choice {}", declaration.name.text));
        self.nested(|printer| {
            for variant in &declaration.variants {
                let fields: Vec<String> = variant.fields.iter().map(field_text).collect();
                let payload = if fields.is_empty() {
                    String::new()
                } else {
                    format!("({})", fields.join(", "))
                };
                printer.line(&format!("variant {}{payload}", variant.name.text));
            }
        });
    }

    fn block(&mut self, block: &Block<'_>) {
        self.nested(|printer| {
            for statement in &block.statements {
                printer.statement(statement);
            }
        });
    }

    fn statement(&mut self, statement: &Statement<'_>) {
        match &statement.kind {
            StatementKind::Binding(binding) => self.binding("", binding),
            StatementKind::Assignment { target, value } => {
                self.value_line(&format!("assign {} =", expr_text(target)), value);
            }
            StatementKind::Expression(expr) => self.value_line("expression", expr),
            StatementKind::If(statement) => self.if_statement(statement),
            StatementKind::While { condition, body } => {
                self.line(&format!("while {}", expr_text(condition)));
                self.block(body);
            }
            StatementKind::For {
                variable,
                iterable,
                body,
            } => {
                self.line(&format!("for {} in {}", variable.text, expr_text(iterable)));
                self.block(body);
            }
            StatementKind::Match { scrutinee, arms } => {
                self.line(&format!("match {}", expr_text(scrutinee)));
                self.nested(|printer| arms.iter().for_each(|arm| printer.match_arm(arm)));
            }
            StatementKind::Return(Some(value)) => self.value_line("return", value),
            StatementKind::Return(None) => self.line("return"),
            StatementKind::Break => self.line("break"),
            StatementKind::Continue => self.line("continue"),
            StatementKind::Assert(condition) => {
                self.line(&format!("assert {}", expr_text(condition)))
            }
        }
    }

    fn binding(&mut self, visibility: &str, binding: &Binding<'_>) {
        let keyword = match binding.mutability {
            Mutability::Constant => "constant",
            Mutability::Variable => "variable",
        };
        let ty = binding
            .ty
            .as_ref()
            .map(|ty| format!("{} ", type_text(ty)))
            .unwrap_or_default();
        self.value_line(
            &format!("{visibility}{keyword} {ty}{} =", binding.name.text),
            &binding.value,
        );
    }

    fn if_statement(&mut self, statement: &IfStatement<'_>) {
        for (index, branch) in statement.branches.iter().enumerate() {
            let keyword = if index == 0 { "if" } else { "else if" };
            self.line(&format!("{keyword} {}", expr_text(&branch.condition)));
            self.block(&branch.body);
        }
        if let Some(otherwise) = &statement.otherwise {
            self.line("else");
            self.block(otherwise);
        }
    }

    fn match_arm(&mut self, arm: &MatchArm<'_>) {
        let bindings: Vec<&str> = arm.pattern.bindings.iter().map(|name| name.text).collect();
        let payload = if bindings.is_empty() {
            String::new()
        } else {
            format!("({})", bindings.join(", "))
        };
        self.line(&format!("case {}{payload}", arm.pattern.variant.text));
        self.block(&arm.body);
    }

    /// Prints a statement that ends in an expression, followed by that expression's `with` block.
    fn value_line(&mut self, prefix: &str, value: &Expr<'_>) {
        let block = block_argument(value);
        let with = block.map(with_text).unwrap_or_default();
        self.line(&format!("{prefix} {}{with}", expr_text(value)));
        if let Some(block) = block {
            self.block(&block.body);
        }
    }
}

fn block_argument<'a, 'src>(expr: &'a Expr<'src>) -> Option<&'a BlockArgument<'src>> {
    match &expr.kind {
        ExprKind::Call(call) => call.block.as_ref(),
        _ => None,
    }
}

fn with_text(block: &BlockArgument<'_>) -> String {
    let parameters: Vec<&str> = block.parameters.iter().map(|name| name.text).collect();
    if parameters.is_empty() {
        " with".to_owned()
    } else {
        format!(" with {}", parameters.join(", "))
    }
}

fn directive_text(directive: &Directive) -> String {
    let words = match directive.kind {
        DirectiveKind::Edition(Edition::V0_1) => "edition 0.1",
        DirectiveKind::Optimize(OptimizeMode::Speed) => "optimize speed",
        DirectiveKind::Optimize(OptimizeMode::Size) => "optimize size",
        DirectiveKind::Optimize(OptimizeMode::Debug) => "optimize debug",
        DirectiveKind::ProveBounds => "prove bounds",
    };
    format!("directive {words}")
}

fn path_text(path: &Path<'_>) -> String {
    let segments: Vec<&str> = path.segments.iter().map(|name| name.text).collect();
    segments.join(".")
}

fn type_text(ty: &TypeExpr<'_>) -> String {
    match &ty.kind {
        TypeExprKind::Packed(inner) => format!("packed {}", type_text(inner)),
        TypeExprKind::Named { path, arguments } if arguments.is_empty() => path_text(path),
        TypeExprKind::Named { path, arguments } => {
            let arguments: Vec<String> = arguments.iter().map(type_argument_text).collect();
            format!("{}[{}]", path_text(path), arguments.join(", "))
        }
    }
}

fn type_argument_text(argument: &TypeArgument<'_>) -> String {
    match argument {
        TypeArgument::Type(ty) => type_text(ty),
        TypeArgument::Size { value, .. } => value.to_string(),
    }
}

fn parameter_text(parameter: &Parameter<'_>) -> String {
    let variable = match parameter.mutability {
        Mutability::Variable => "variable ",
        Mutability::Constant => "",
    };
    format!(
        "{variable}{} {}",
        type_text(&parameter.ty),
        parameter.name.text
    )
}

fn field_text(field: &Field<'_>) -> String {
    format!("{} {}", type_text(&field.ty), field.name.text)
}

fn expr_text(expr: &Expr<'_>) -> String {
    match &expr.kind {
        ExprKind::Integer(value) => value.to_string(),
        ExprKind::Float(value) => format!("{value:?}"),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Name(name) => (*name).to_owned(),
        ExprKind::Text(parts) => {
            let mut words = vec!["text".to_owned()];
            words.extend(parts.iter().map(text_part_text));
            format!("({})", words.join(" "))
        }
        ExprKind::Unary { operator, operand } => {
            format!("({} {})", operator.symbol(), expr_text(operand))
        }
        ExprKind::Binary {
            operator,
            left,
            right,
        } => {
            format!(
                "({} {} {})",
                operator.symbol(),
                expr_text(left),
                expr_text(right)
            )
        }
        ExprKind::Call(call) => {
            let mut words = vec!["call".to_owned(), expr_text(&call.callee)];
            words.extend(call.arguments.iter().map(argument_text));
            format!("({})", words.join(" "))
        }
        ExprKind::Field { object, field } => format!("(. {} {})", expr_text(object), field.text),
        ExprKind::Index { object, index } => {
            format!("(index {} {})", expr_text(object), expr_text(index))
        }
    }
}

fn text_part_text(part: &TextPart<'_>) -> String {
    match part {
        TextPart::Literal(text) => format!("{text:?}"),
        TextPart::Interpolation(expr) => expr_text(expr),
    }
}

fn argument_text(argument: &Argument<'_>) -> String {
    match &argument.kind {
        ArgumentKind::Positional(value) => expr_text(value),
        ArgumentKind::Named { name, value } => format!("{}={}", name.text, expr_text(value)),
        ArgumentKind::Mutable(place) => format!("(variable {})", expr_text(place)),
    }
}
