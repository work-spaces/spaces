use anyhow::Context;
use anyhow_source_location::format_context;
use serde::Serialize;
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
    /// the output easily readable by humans. The default indentation is 2 spaces.
    /// Use `to_string_indented` if you need a different indentation width.
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
    /// * `str`: The formatted, multi-line JSON string indented with 2 spaces.
    fn to_string_pretty(value: starlark::values::Value) -> anyhow::Result<String> {
        let json_string = serde_json::to_string_pretty(&value.to_json_value()?)
            .context(format_context!("Failed to convert dict to json string"))?;

        Ok(json_string)
    }

    /// Converts a dictionary or Starlark value into a pretty-printed JSON string
    /// with a configurable indentation width.
    ///
    /// This is the flexible alternative to `to_string_pretty`, allowing you to
    /// control exactly how many spaces are used for each indentation level.
    /// Use `indent = 0` to get compact output with newlines but no indentation
    /// (unusual but valid). NaN and Infinity values are rejected with an error.
    ///
    /// ```python
    /// data = {"key": "value", "nums": [1, 2, 3]}
    /// # 4-space indentation
    /// wide = json.to_string_indented(data, indent = 4)
    /// # 1-space indentation
    /// narrow = json.to_string_indented(data, indent = 1)
    /// ```
    ///
    /// # Arguments
    /// * `value`: The dictionary or Starlark value to be serialized.
    /// * `indent`: Number of spaces to use per indentation level (0–16).
    ///
    /// # Returns
    /// * `str`: The formatted, multi-line JSON string with the requested indentation.
    fn to_string_indented(
        value: starlark::values::Value,
        #[starlark(require = named)] indent: i32,
    ) -> anyhow::Result<String> {
        if indent < 0 || indent > 16 {
            return Err(anyhow::anyhow!(
                "indent must be between 0 and 16, got {}",
                indent
            ));
        }
        let json_value = value
            .to_json_value()
            .context(format_context!("Failed to convert value to JSON"))?;

        let indent_bytes = " ".repeat(indent as usize);
        let formatter = serde_json::ser::PrettyFormatter::with_indent(indent_bytes.as_bytes());
        let mut buf = Vec::new();
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        json_value.serialize(&mut ser).context(format_context!(
            "Failed to serialize JSON with indent {}",
            indent
        ))?;

        String::from_utf8(buf).context(format_context!("Serialized JSON contained invalid UTF-8"))
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

    /// Tries to convert a JSON formatted string into a Starlark dictionary/value.
    ///
    /// On success, returns the parsed Starlark value.  On failure, returns the
    /// value supplied to `default` (which itself defaults to `None`).
    ///
    /// This is preferred over calling `is_string_json` followed by
    /// `string_to_dict` because it parses the input only once.  A named
    /// `default` parameter lets callers supply a sentinel that is
    /// distinguishable from a successfully decoded JSON `null`:
    ///
    /// ```python
    /// MISSING = "PARSE_FAILED"
    /// result = json.try_string_to_dict(raw, default = MISSING)
    /// if result == MISSING:
    ///     print("input was not valid JSON")
    /// elif result == None:
    ///     print("input was the JSON literal null")
    /// else:
    ///     print(result["key"])
    /// ```
    ///
    /// When called without `default` the behaviour is identical to the
    /// original: `None` is returned on parse failure.
    ///
    /// ```python
    /// raw_data = '{"id": 101, "status": "active"}'
    /// result = json.try_string_to_dict(raw_data)
    /// if result != None:
    ///     print(result["status"])
    /// else:
    ///     print("Failed to parse JSON")
    /// ```
    ///
    /// # Arguments
    /// * `content`: The JSON-formatted string to be converted.
    /// * `default` *(named, optional)*: Value to return when parsing fails.
    ///   Defaults to `None`.
    ///
    /// # Returns
    /// * `dict | <default>`: A Starlark value representing the parsed JSON, or
    ///   the `default` value if parsing fails.
    fn try_string_to_dict<'v>(
        content: &str,
        #[starlark(require = named)] default: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        match serde_json::from_str::<serde_json::Value>(content) {
            Ok(json_value) => Ok(heap.alloc(json_value)),
            Err(_) => Ok(default.unwrap_or_else(Value::new_none)),
        }
    }
}
