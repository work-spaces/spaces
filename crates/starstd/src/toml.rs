use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Converts a TOML formatted string into a Starlark dictionary/value.
    ///
    /// ```python
    /// raw_data = 'id = 101\nstatus = "active"'
    /// data_dict = toml.string_to_dict(raw_data)
    /// print(data_dict["status"])
    /// ```
    ///
    /// # Arguments
    /// * `content`: The TOML-formatted string to be converted.
    ///
    /// # Returns
    /// * `dict`: A dictionary representation of the TOML data.
    fn string_to_dict<'v>(
        content: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let toml_value: toml::Value =
            toml::from_str(content).context(format_context!("bad toml string"))?;

        // Convert TOML value into JSON value first, then allocate as Starlark value.
        let json_value = serde_json::to_value(toml_value)
            .context(format_context!("Failed to convert toml to json value"))?;
        let alloc_value = heap.alloc(json_value);

        Ok(alloc_value)
    }

    /// Converts a dictionary or Starlark value into a TOML-formatted string.
    ///
    /// ```python
    /// data = {"name": "Project Alpha", "version": 1}
    /// toml_string = toml.to_string(data)
    /// ```
    ///
    /// # Arguments
    /// * `value`: The dictionary or Starlark value to be serialized.
    ///
    /// # Returns
    /// * `str`: The TOML string representation of the input value.
    fn to_string(value: starlark::values::Value) -> anyhow::Result<String> {
        let toml_string = toml::to_string(&value.to_json_value()?)
            .context(format_context!("Failed to convert dict to toml string"))?;
        Ok(toml_string)
    }

    /// Converts a dictionary or Starlark value into a pretty-printed TOML string.
    ///
    /// ```python
    /// data = {"project": "Gemini", "active": True, "tags": ["ai", "helper"]}
    /// pretty_toml = toml.to_string_pretty(data)
    /// print(pretty_toml)
    /// ```
    ///
    /// # Arguments
    /// * `value`: The dictionary or Starlark value to be serialized.
    ///
    /// # Returns
    /// * `str`: The formatted TOML string.
    fn to_string_pretty(value: starlark::values::Value) -> anyhow::Result<String> {
        let pretty_toml = toml::to_string_pretty(&value.to_json_value()?).context(
            format_context!("Failed to convert dict to pretty toml string"),
        )?;
        Ok(pretty_toml)
    }

    /// Attempts to convert a TOML formatted string into a Starlark dictionary/value.
    /// Returns a Starlark None value if parsing fails instead of propagating an error.
    ///
    /// ```python
    /// raw_data = 'id = 101\nstatus = "active"'
    /// result = toml.try_string_to_dict(raw_data)
    /// if result != None:
    ///     print(result["status"])
    /// ```
    ///
    /// # Arguments
    /// * `content`: The TOML-formatted string to be converted.
    ///
    /// # Returns
    /// * `dict | None`: A dictionary representation of the TOML data, or None if parsing fails.
    fn try_string_to_dict<'v>(
        content: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        match toml::from_str::<toml::Value>(content) {
            Ok(toml_value) => {
                let json_value = serde_json::to_value(toml_value)
                    .context(format_context!("Failed to convert toml to json value"))?;
                let alloc_value = heap.alloc(json_value);
                Ok(alloc_value)
            }
            Err(_) => Ok(heap.alloc(serde_json::Value::Null)),
        }
    }
}
