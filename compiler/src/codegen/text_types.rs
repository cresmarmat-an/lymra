/*!
 * The C functions that walk the text inside a value, for every type that
 * holds text, and the lists and stacks whose elements hold text.
 *
 * A value that holds text holds handles to its bytes, which lie on a data
 * stack that whatever holds the value owns. `own` copies each piece of text
 * to the top of a data stack and points the value at the copy, unless the
 * program itself keeps that text; `rebase` points the pieces that moved down
 * a data stack at their new place; `lowest` finds where the lowest piece on
 * a data stack starts, and `highest` where the highest one ends. A list or
 * a stack of such values keeps their text on a second data stack, in the
 * order of its elements.
 */

use super::INDENT;
use super::c_syntax;
use super::types::CTypes;
use crate::ir::{ArrayType, ChoiceType, CustomType, Type};

/// What a walk over the text in a value does with each piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TextWalk {
    Own,
    Rebase,
    Lowest,
    Highest,
}

impl TextWalk {
    const ALL: [TextWalk; 4] = [
        TextWalk::Own,
        TextWalk::Rebase,
        TextWalk::Lowest,
        TextWalk::Highest,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Rebase => "rebase",
            Self::Lowest => "lowest",
            Self::Highest => "highest",
        }
    }

    /// The C statement that walks the text in `place`, whose walk is `function`.
    fn apply(self, function: &str, place: &str) -> String {
        match self {
            Self::Own => format!("{function}(&{place}, stack, site);"),
            Self::Rebase => format!("{function}(&{place}, from, to, distance);"),
            Self::Lowest => format!("lowest = {function}(&{place}, stack, lowest);"),
            Self::Highest => format!("highest = {function}(&{place}, stack, highest);"),
        }
    }

    /// The declaration of the walk for values of the C type `c_type`.
    fn signature(self, c_type: &str) -> String {
        let function = walk_name(c_type, self);
        match self {
            Self::Own => format!(
                "LYMRA_UNUSED static void {function}({c_type} *value, lymra_data_stack *stack, const char *site)"
            ),
            Self::Rebase => format!(
                "LYMRA_UNUSED static void {function}({c_type} *value, const char *from, const char *to, size_t distance)"
            ),
            Self::Lowest => format!(
                "LYMRA_UNUSED static char *{function}(const {c_type} *value, const lymra_data_stack *stack, char *lowest)"
            ),
            Self::Highest => format!(
                "LYMRA_UNUSED static char *{function}(const {c_type} *value, const lymra_data_stack *stack, char *highest)"
            ),
        }
    }
}

fn walk_name(c_type: &str, walk: TextWalk) -> String {
    format!("{c_type}_{}", walk.name())
}

/// The C function that walks the text in a value of `ty`, which holds text, as `walk` says.
pub(super) fn text_function(types: &mut CTypes, ty: &Type, walk: TextWalk) -> String {
    match ty {
        Type::String => format!("lymra_text_{}", walk.name()),
        other => walk_name(&types.name(other), walk),
    }
}

/// The statements that walk the text in `place`, of type `ty`, if it holds any.
fn walk_place(types: &mut CTypes, ty: &Type, place: &str, walk: TextWalk) -> Option<String> {
    ty.holds_text()
        .then(|| walk.apply(&text_function(types, ty, walk), place))
}

/// The walks for `c_type`, given the statements each one runs on a value. They need the runtime's data stacks.
fn walks(
    types: &mut CTypes,
    c_type: &str,
    mut body: impl FnMut(&mut CTypes, TextWalk) -> Vec<String>,
) -> String {
    types.uses_data_stacks = true;
    TextWalk::ALL
        .iter()
        .map(|&walk| {
            let mut lines: Vec<String> = body(types, walk)
                .into_iter()
                .map(|line| format!("{INDENT}{line}"))
                .collect();
            match walk {
                TextWalk::Lowest => lines.push(format!("{INDENT}return lowest;")),
                TextWalk::Highest => lines.push(format!("{INDENT}return highest;")),
                TextWalk::Own | TextWalk::Rebase => {}
            }
            format!("{} {{\n{}\n}}", walk.signature(c_type), lines.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The walks of a declared type, whose struct is `c_type`: each field that holds text, in order.
pub(super) fn custom_walks(types: &mut CTypes, c_type: &str, custom: &CustomType) -> String {
    walks(types, c_type, |types, walk| {
        custom
            .fields
            .iter()
            .enumerate()
            .filter_map(|(position, field)| {
                let place = format!("value->{}", c_syntax::field_name(position));
                walk_place(types, &field.ty, &place, walk)
            })
            .collect()
    })
}

/// The walks of an array type, whose struct is `c_type`: each element, in order.
pub(super) fn array_walks(types: &mut CTypes, c_type: &str, array: &ArrayType) -> String {
    walks(types, c_type, |types, walk| {
        let Some(statement) = walk_place(types, &array.element, "value->elements[index]", walk)
        else {
            return Vec::new();
        };
        vec![
            format!(
                "for (size_t index = 0; index < {}u; ++index) {{",
                array.length
            ),
            format!("{INDENT}{statement}"),
            "}".to_owned(),
        ]
    })
}

/// The walks of a choice, whose struct is `c_type`: the fields that hold text of the variant it holds.
pub(super) fn choice_walks(types: &mut CTypes, c_type: &str, choice: &ChoiceType) -> String {
    walks(types, c_type, |types, walk| {
        let mut lines = vec![format!("switch (value->{}) {{", c_syntax::TAG)];
        for (variant_position, variant) in choice.variants.iter().enumerate() {
            let statements: Vec<String> = variant
                .fields
                .iter()
                .enumerate()
                .filter_map(|(position, field)| {
                    let place = format!(
                        "value->{}.{}.{}",
                        c_syntax::VARIANTS,
                        c_syntax::variant_name(variant_position),
                        c_syntax::field_name(position)
                    );
                    walk_place(types, &field.ty, &place, walk)
                })
                .collect();
            if statements.is_empty() {
                continue;
            }
            lines.push(format!("case {variant_position}:"));
            lines.extend(
                statements
                    .into_iter()
                    .map(|statement| format!("{INDENT}{statement}")),
            );
            lines.push(format!("{INDENT}break;"));
        }
        lines.push("default:".to_owned());
        lines.push(format!("{INDENT}break;"));
        lines.push("}".to_owned());
        lines
    })
}

/**
 * The struct of a list or a stack whose elements hold text, and the
 * functions that work on it. Besides its elements, it keeps where its text
 * starts and the data stack the text grows on. `push` copies an element's
 * text after the text of the elements before it; `place` copies the list to
 * the top of two stacks, its text in the order of its elements, so a list
 * whose text lies on the same stack, above, moves down safely; `replace`
 * moves a list built right above another one down into its place, its text
 * too.
 */
pub(super) fn text_list_definition(
    types: &mut CTypes,
    name: &str,
    element: &str,
    element_type: &Type,
) -> String {
    let own = text_function(types, element_type, TextWalk::Own);
    let rebase = text_function(types, element_type, TextWalk::Rebase);
    format!(
        "/* list[{element_type}] */
typedef struct {{
    {element} *elements;
    int64_t length;
    lymra_data_stack *stack;
    char *text;
    lymra_data_stack *text_stack;
}} {name};

LYMRA_UNUSED static {name} {name}_start(lymra_data_stack *stack, lymra_data_stack *text_stack) {{
    {name} list;
    list.elements = ({element} *)(void *)lymra_data_stack_align(stack, _Alignof({element}));
    list.length = 0;
    list.stack = stack;
    list.text = text_stack->top;
    list.text_stack = text_stack;
    return list;
}}

LYMRA_UNUSED static void {name}_push({name} *list, const {element} value, const char *site) {{
    char *end = (char *)(list->elements + list->length);
    {element} *slot = ({element} *)(void *)lymra_data_stack_extend(list->stack, end, sizeof value, site);
    *slot = value;
    {own}(slot, list->text_stack, site);
    list->length += 1;
}}

LYMRA_UNUSED static {name} {name}_place(lymra_data_stack *stack, lymra_data_stack *text_stack, const {name} source,
                                          const char *site) {{
    {name} list;
    size_t bytes = (size_t)source.length * sizeof *source.elements;
    list.elements = ({element} *)(void *)lymra_data_stack_place(stack, source.elements, bytes, _Alignof({element}),
                                                                 site);
    list.length = source.length;
    list.stack = stack;
    list.text = text_stack->top;
    list.text_stack = text_stack;
    for (int64_t index = 0; index < list.length; ++index) {{
        {own}(&list.elements[index], text_stack, site);
    }}
    return list;
}}

LYMRA_UNUSED static void {name}_replace({name} *list, const {name} value) {{
    if (value.length != 0) {{
        memmove(list->elements, value.elements, (size_t)value.length * sizeof *value.elements);
    }}
    list->length = value.length;
    lymra_data_stack_release(list->stack, (char *)(list->elements + value.length));
    char *to = list->text_stack->top;
    size_t distance = lymra_data_stack_lower(list->text_stack, list->text, value.text);
    for (int64_t index = 0; index < list->length; ++index) {{
        {rebase}(&list->elements[index], value.text, to, distance);
    }}
}}"
    )
}

/**
 * `pop` and `top` for a stack whose elements hold text. `pop` copies the
 * text of the top element onto `keep`, the data stack of the value it gives,
 * then frees that text, which lies after the text of every element below.
 */
pub(super) fn text_stack_definition(
    types: &mut CTypes,
    name: &str,
    element: &str,
    element_type: &Type,
) -> String {
    let own = text_function(types, element_type, TextWalk::Own);
    let lowest = text_function(types, element_type, TextWalk::Lowest);
    format!(
        "LYMRA_UNUSED static {element} {name}_pop({name} *stack, lymra_data_stack *keep, const char *site) {{
    lymra_data_stack_require_top(stack->stack, (const char *)(stack->elements + stack->length), site);
    if (stack->length == 0) {{
        lymra_trap(LYMRA_EMPTY_STACK, site);
    }}
    stack->length -= 1;
    {element} value = stack->elements[stack->length];
    char *start = {lowest}(&value, stack->text_stack, NULL);
    {own}(&value, keep, site);
    if (start != NULL) {{
        lymra_data_stack_release(stack->text_stack, start);
    }}
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
