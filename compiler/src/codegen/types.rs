/*!
 * The C name of every type. Each array type and each declared type becomes
 * a struct of its own, so they are values in C as they are in Lymra:
 * assigned, passed and returned whole. A struct is defined the first time a
 * type is named, after the structs of the values it holds.
 *
 * A list type becomes a struct that says where its elements start on their
 * data stack, how many there are, and which stack they grow on, with the
 * functions that build, grow and replace such a list. A view type becomes a
 * struct that says where the elements it shows start and how many there
 * are, which it can only read.
 */

use std::collections::HashMap;
use std::sync::Arc;

use super::{INDENT, c_syntax};
use crate::ir::{ArrayType, ChoiceType, CustomType, FloatType, ListType, Sequence, Type, ViewType};

#[derive(Default)]
pub(super) struct CTypes {
    arrays: HashMap<Arc<ArrayType>, String>,
    lists: HashMap<Arc<ListType>, String>,
    views: HashMap<Arc<ViewType>, String>,
    /// The declared types and choices defined so far, by name, which is unique in a program.
    customs: HashMap<String, String>,
    /// The structs, and the fill functions of arrays, in the order C needs them.
    definitions: Vec<String>,
    /// Whether a float type has been named, so the program uses floats.
    pub uses_floats: bool,
    /// Whether a list type has been named, so the program needs the runtime's data stacks.
    pub uses_lists: bool,
}

impl CTypes {
    /// The C name of `ty`. Naming an array type for the first time defines its struct.
    pub fn name(&mut self, ty: &Type) -> String {
        match ty {
            Type::Integer(integer) => c_syntax::integer_type(*integer).to_owned(),
            Type::Float(float) => {
                self.uses_floats = true;
                match float {
                    FloatType::Float32 => "float".to_owned(),
                    FloatType::Float64 => "double".to_owned(),
                }
            }
            Type::Boolean => "bool".to_owned(),
            Type::String => "lymra_string".to_owned(),
            Type::Array(array) => self.array_name(array),
            Type::Custom(custom) => self.custom_name(custom),
            Type::Choice(choice) => self.choice_name(choice),
            Type::List(list) => self.list_name(list),
            Type::View(view) => self.view_name(view),
        }
    }

    /**
     * The struct of a declared type. Its members are named by position, as
     * `field_0`, so a field is reached from its position alone; a comment
     * gives each its Lymra name.
     */
    fn custom_name(&mut self, custom: &CustomType) -> String {
        if let Some(name) = self.customs.get(&custom.name) {
            return name.clone();
        }
        let members = self.members(custom, INDENT);
        let name = c_syntax::custom_type_name(&custom.name);
        self.definitions.push(format!(
            "/* {} */\ntypedef struct {{\n{members}\n}} {name};",
            custom.name
        ));
        self.customs.insert(custom.name.clone(), name.clone());
        name
    }

    /**
     * The struct of a choice: a tag that says which variant the value holds,
     * then a union with a struct for each variant that has fields. Its
     * members are named by position too, as `variant_0`.
     */
    fn choice_name(&mut self, choice: &ChoiceType) -> String {
        if let Some(name) = self.customs.get(&choice.name) {
            return name.clone();
        }
        let tag = c_syntax::integer_type(choice.tag_type());
        let nested = format!("{INDENT}{INDENT}{INDENT}");
        let variants: Vec<String> = choice
            .variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| !variant.fields.is_empty())
            .map(|(position, variant)| {
                let members = self.members(variant, &nested);
                format!(
                    "{INDENT}{INDENT}struct {{ /* {} */\n{members}\n{INDENT}{INDENT}}} {};",
                    variant.name,
                    c_syntax::variant_name(position)
                )
            })
            .collect();
        // C has no empty union, so a choice whose variants hold nothing is its tag alone.
        let payload = if variants.is_empty() {
            String::new()
        } else {
            format!(
                "\n{INDENT}union {{\n{}\n{INDENT}}} {};",
                variants.join("\n"),
                c_syntax::VARIANTS
            )
        };
        let name = c_syntax::choice_name(&choice.name);
        self.definitions.push(format!(
            "/* {} */\ntypedef struct {{\n{INDENT}{tag} {};{payload}\n}} {name};",
            choice.name,
            c_syntax::TAG
        ));
        self.customs.insert(choice.name.clone(), name.clone());
        name
    }

    /// The members of a struct for `fields`, one a line, each with a comment that gives its Lymra name.
    fn members(&mut self, fields: &CustomType, indent: &str) -> String {
        let members: Vec<String> = fields
            .fields
            .iter()
            .enumerate()
            .map(|(position, field)| {
                let ty = self.name(&field.ty);
                format!(
                    "{indent}{ty} {}; /* {} */",
                    c_syntax::field_name(position),
                    field.name
                )
            })
            .collect();
        members.join("\n")
    }

    fn array_name(&mut self, array: &Arc<ArrayType>) -> String {
        if let Some(name) = self.arrays.get(array) {
            return name.clone();
        }
        let element = self.name(&array.element);
        let name = format!("array_{}", self.arrays.len());
        self.definitions.push(definition(&name, &element, array));
        self.arrays.insert(Arc::clone(array), name.clone());
        name
    }

    fn list_name(&mut self, list: &Arc<ListType>) -> String {
        if let Some(name) = self.lists.get(list) {
            return name.clone();
        }
        let element = self.name(&list.element);
        let name = format!("{}_{}", list.kind.name(), self.lists.len());
        self.definitions
            .push(list_definition(&name, &element, &list.element));
        if list.kind == Sequence::Stack {
            self.definitions.push(stack_definition(&name, &element));
        }
        self.lists.insert(Arc::clone(list), name.clone());
        self.uses_lists = true;
        name
    }

    fn view_name(&mut self, view: &Arc<ViewType>) -> String {
        if let Some(name) = self.views.get(view) {
            return name.clone();
        }
        let element = self.name(&view.element);
        let name = format!("view_{}", self.views.len());
        self.definitions.push(format!(
            "/* view[{}] */\ntypedef struct {{\n{INDENT}const {element} *elements;\n{INDENT}int64_t length;\n}} {name};",
            view.element
        ));
        self.views.insert(Arc::clone(view), name.clone());
        name
    }

    /// Every struct named so far, each after the ones it holds.
    pub fn definitions(&self) -> &[String] {
        &self.definitions
    }
}

/// The function that does `operation` (`start`, `push`, `place` or `replace`) on lists of the C type `name`.
pub(super) fn list_function(name: &str, operation: &str) -> String {
    format!("{name}_{operation}")
}

/**
 * The struct for a list type and the functions that work on it. `start`
 * begins an empty list at the top of a stack; `push` adds to the end of a
 * list, which has to be the newest value on its stack; `place` copies
 * elements to the top of a stack, from anywhere, the same stack included;
 * `replace` moves a list built right above another one down into its place.
 */
fn list_definition(name: &str, element: &str, element_type: &Type) -> String {
    format!(
        "/* list[{element_type}] */
typedef struct {{
    {element} *elements;
    int64_t length;
    lymra_data_stack *stack;
}} {name};

LYMRA_UNUSED static {name} {name}_start(lymra_data_stack *stack) {{
    {name} list;
    list.elements = ({element} *)(void *)lymra_data_stack_align(stack, _Alignof({element}));
    list.length = 0;
    list.stack = stack;
    return list;
}}

LYMRA_UNUSED static void {name}_push({name} *list, const {element} value, const char *site) {{
    char *end = (char *)(list->elements + list->length);
    {element} *slot = ({element} *)(void *)lymra_data_stack_extend(list->stack, end, sizeof value, site);
    *slot = value;
    list->length += 1;
}}

LYMRA_UNUSED static {name} {name}_place(lymra_data_stack *stack, const {element} *elements, int64_t length,
                                          const char *site) {{
    {name} list;
    size_t bytes = (size_t)length * sizeof *elements;
    list.elements = ({element} *)(void *)lymra_data_stack_place(stack, elements, bytes, _Alignof({element}), site);
    list.length = length;
    list.stack = stack;
    return list;
}}

LYMRA_UNUSED static void {name}_replace({name} *list, const {name} value) {{
    if (value.length != 0) {{
        memmove(list->elements, value.elements, (size_t)value.length * sizeof *value.elements);
    }}
    list->length = value.length;
    lymra_data_stack_release(list->stack, (char *)(list->elements + value.length));
}}"
    )
}

/**
 * The functions that only a stack has: `pop` takes the top element off
 * and frees its place, and `top` reads it. Either stops the program when
 * the stack is empty. A stack is a list in C, so the list's functions build
 * and grow it.
 */
fn stack_definition(name: &str, element: &str) -> String {
    format!(
        "LYMRA_UNUSED static {element} {name}_pop({name} *stack, const char *site) {{
    lymra_data_stack_require_top(stack->stack, (const char *)(stack->elements + stack->length), site);
    if (stack->length == 0) {{
        lymra_trap(LYMRA_EMPTY_STACK, site);
    }}
    stack->length -= 1;
    {element} value = stack->elements[stack->length];
    lymra_data_stack_shrink(stack->stack, (char *)(stack->elements + stack->length), sizeof value);
    return value;
}}

LYMRA_UNUSED static {element} {name}_top(const {name} *stack, const char *site) {{
    if (stack->length == 0) {{
        lymra_trap(LYMRA_EMPTY_STACK, site);
    }}
    return stack->elements[stack->length - 1];
}}"
    )
}

/// The function that builds an array of type `name` with every element set to one value.
pub(super) fn fill_function(name: &str) -> String {
    format!("{name}_filled")
}

/**
 * The struct for an array type, and its fill function. The fill function
 * sets the first element and then doubles the filled part with `memcpy`.
 * A loop that stores the value into each element is one C compilers
 * rewrite, and MSVC rewrites it for large arrays into a fill of bytes,
 * which is wrong for every element larger than a byte.
 */
fn definition(name: &str, element: &str, array: &ArrayType) -> String {
    let length = array.length;
    let fill = fill_function(name);
    format!(
        "/* array[{element_type}, {length}] */
typedef struct {{
    {element} elements[{length}];
}} {name};

LYMRA_UNUSED static {name} {fill}(const {element} value) {{
    {name} filled;
    size_t done = 1;
    filled.elements[0] = value;
    while (done < {length}u) {{
        size_t step = done < {length}u - done ? done : {length}u - done;
        memcpy(&filled.elements[done], &filled.elements[0], step * sizeof filled.elements[0]);
        done += step;
    }}
    return filled;
}}",
        element_type = array.element,
    )
}
