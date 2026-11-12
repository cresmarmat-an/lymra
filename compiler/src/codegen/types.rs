/*!
 * The C name of every type. Each array type becomes a struct of its own, so
 * arrays are values in C as they are in Lymra: assigned, passed and returned
 * whole. A struct is defined the first time a type is named, after the
 * structs of the arrays it holds.
 */

use std::collections::HashMap;
use std::sync::Arc;

use super::c_syntax;
use crate::ir::{ArrayType, Type};

#[derive(Default)]
pub(super) struct CTypes {
    arrays: HashMap<Arc<ArrayType>, String>,
    /// The struct and fill function of each array type, in the order C needs them.
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
        }
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

    /// Every array struct named so far, each after the ones it holds.
    pub fn definitions(&self) -> &[String] {
        &self.definitions
    }
}

/// The function that builds an array of type `name` with every element set to one value.
pub(super) fn fill_function(name: &str) -> String {
    format!("{name}_filled")
}

/// The struct for an array type, and its fill function.
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
    for (size_t position = 0; position < {length}u; ++position) {{
        filled.elements[position] = value;
    }}
    return filled;
}}",
        element_type = array.element,
    )
}
