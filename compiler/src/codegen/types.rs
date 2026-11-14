/*!
 * The C name of every type. Each array type and each declared type becomes
 * a struct of its own, so they are values in C as they are in Lymra:
 * assigned, passed and returned whole. A struct is defined the first time a
 * type is named, after the structs of the values it holds.
 */

use std::collections::HashMap;
use std::sync::Arc;

use super::c_syntax;
use crate::ir::{ArrayType, CustomType, Type};

#[derive(Default)]
pub(super) struct CTypes {
    arrays: HashMap<Arc<ArrayType>, String>,
    /// The declared types defined so far, by name, which is unique in a program.
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
        let members: Vec<String> = custom
            .fields
            .iter()
            .enumerate()
            .map(|(position, field)| {
                let ty = self.name(&field.ty);
                format!(
                    "    {ty} {}; /* {} */",
                    c_syntax::field_name(position),
                    field.name
                )
            })
            .collect();
        let name = c_syntax::custom_type_name(&custom.name);
        self.definitions.push(format!(
            "/* {} */\ntypedef struct {{\n{}\n}} {name};",
            custom.name,
            members.join("\n")
        ));
        self.customs.insert(custom.name.clone(), name.clone());
        name
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
