use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Converts a JSON formatted string into a Python dictionary.
    ///
    /// This function acts as a parser, taking a raw JSON string and
    /// transforming it into a structured dictionary that you can
    /// easily manipulate in your scripts.
    ///
    /// ```python
    /// raw_data = '{"id": 101, "status": "active"}'
    /// data_dict = json.string_to_dict(raw_data)
    /// print(data_dict["status"])
    /// ```
    ///
    /// # Arguments
    /// * `content`: The JSON-formatted string to be converted.
    ///
    /// # Returns
    /// * `dict`: A dictionary representation of the JSON data.
    fn string_to_dict<'v>(
        content: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let json_value: serde_json::Value =
            serde_json::from_str(content).context(format_context!("bad json string"))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);

        Ok(alloc_value)
    }

    /// Converts a dictionary or Starlark value into a JSON-formatted string.
    ///
    /// This is the inverse of `string_to_dict`. It takes structured data
    /// and serializes it into a string, making it ready to be written
    /// to a file or sent over a network.
    ///
    /// ```python
    /// data = {"name": "Project Alpha", "version": 1}
    /// json_string = json.to_string(data)
    /// ```
    ///
    /// # Arguments
    /// * `value`: The dictionary or Starlark value to be serialized.
    ///
    /// # Returns
    /// * `str`: The JSON string representation of the input value.
    fn to_string(value: starlark::values::Value) -> anyhow::Result<String> {
        let json_string = serde_json::to_string(&value.to_json_value()?)
            .context(format_context!("Failed to convert dict to json string"))?;
        Ok(json_string)
    }

    /// Converts a dictionary or Starlark value into a "pretty-printed" JSON string.
    ///
    /// Unlike `to_string`, this version adds newlines and indentation to make
    /// the output easily readable by humans. This is especially useful for
    /// generating configuration files or debug logs.
    ///
    /// ```python
    /// data = {"project": "Gemini", "active": True, "tags": ["ai", "helper"]}
    /// pretty_json = json.to_string_pretty(data)
    /// print(pretty_json)
    /// ```
    ///
    /// # Arguments
    /// * `value`: The dictionary or Starlark value to be serialized.
    ///
    /// # Returns
    /// * `str`: The formatted, multi-line JSON string.
    fn to_string_pretty(value: starlark::values::Value) -> anyhow::Result<String> {
        let json_string = serde_json::to_string_pretty(&value.to_json_value()?)
            .context(format_context!("Failed to convert dict to json string"))?;

        Ok(json_string)
    }

    /// Returns true if the given string is valid JSON.
    ///
    /// ```python
    /// if json.is_string_json('{"key": "value"}'):
    ///     print("Valid JSON")
    /// ```
    ///
    /// # Arguments
    /// * `value`: The string to check.
    ///
    /// # Returns
    /// * `bool`: True if the string is valid JSON, False otherwise.
    fn is_string_json(value: &str) -> anyhow::Result<bool> {
        Ok(serde_json::from_str::<serde_json::Value>(value).is_ok())
    }
}
