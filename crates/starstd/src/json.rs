use crate::{Arg, Function};
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::{Heap, Value};

pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "string_to_dict",
        description: "Converts a JSON formatted string to a dict.",
        return_type: "dict",
        args: &[Arg {
            name: "content",
            description: "The JSON string to convert",
            dict: &[],
        }],
        example: None,
    },
    Function {
        name: "to_string",
        description: "Converts a dict to a JSON formatted string.",
        return_type: "dict",
        args: &[Arg {
            name: "value",
            description: "The Starlark value to convert",
            dict: &[],
        }],
        example: None,
    },
    Function {
        name: "to_string_pretty",
        description: "Converts a dict to a JSON formatted string (multi-line, idented).",
        return_type: "dict",
        args: &[Arg {
            name: "value",
            description: "The Starlark value to convert",
            dict: &[],
        }],
        example: None,
    },
];

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn string_to_dict<'v>(content: &str, heap: &'v Heap) -> anyhow::Result<Value<'v>> {
        let json_value: serde_json::Value =
            serde_json::from_str(content).context(format_context!("bad json string"))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);

        Ok(alloc_value)
    }

    fn to_string(value: starlark::values::Value) -> anyhow::Result<String> {
        let json_string = serde_json::to_string(&value.to_json_value()?)
            .context(format_context!("Failed to convert dict to json string"))?;
        Ok(json_string)
    }

    fn to_string_pretty(value: starlark::values::Value) -> anyhow::Result<String> {
        let json_string = serde_json::to_string_pretty(&value.to_json_value()?)
            .context(format_context!("Failed to convert dict to json string"))?;

        Ok(json_string)
    }
}
