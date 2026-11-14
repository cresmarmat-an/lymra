/*!
 * The C name of every type. Each array type and each declared type becomes
 * a struct of its own, so they are values in C as they are in Lymra:
 * assigned, passed and returned whole. A struct is defined the first time a
 * type is named, after the structs of the values it holds.
 */

use std::collections::HashMap;
use std::sync::Arc;

use super::{INDENT, c_syntax};
use crate::ir::{ArrayType, ChoiceType, CustomType, Type};

#[derive(Default)]
pub(super) struct CTypes {
    arrays: HashMap<Arc<ArrayType>, String>,
    /// The declared types and choices defined so far, by name, which is unique in a program.
    customs: HashMap<String, String>,
    /// The structs, and the fill functions of arrays, in the order C needs them.
    definitions: Vec<String>,
}

impl CTypes {
    /// The C name of `ty`. Naming an array type for the first time defines its struct.
    pub fn name(&mut self, ty: &Type) -> String {
        match ty {
            Type::Integer(integer) => c_syntax::integer_type(*integer).to_owned(),
            Type::Boolean => "bool".to_owned(),
            Type::String => "lymra_string".to_owned(),
            Type::Array(array) => self.array_name(array),
            Type::Custom(custom) => self.custom_name(custom),
            Type::Choice(choice) => self.choice_name(choice),
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

    /// Every struct named so far, each after the ones it holds.
    pub fn definitions(&self) -> &[String] {
        &self.definitions
    }
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
