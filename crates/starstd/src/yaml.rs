use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Converts a YAML formatted string into a Python dictionary.
    ///
    /// This function acts as a parser, taking a raw YAML string and
    /// transforming it into a structured dictionary that you can
    /// easily manipulate in your scripts.
    ///
    /// ```python
    /// raw_data = "id: 101\nstatus: active\n"
    /// data_dict = yaml.string_to_dict(raw_data)
    /// print(data_dict["status"])
    /// ```
    ///
    /// # Arguments
    /// * `content`: The YAML-formatted string to be converted.
    ///
    /// # Returns
    /// * `dict`: A dictionary representation of the YAML data.
    fn string_to_dict<'v>(
        content: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let yaml_value: serde_yaml::Value =
            serde_yaml::from_str(content).context(format_context!("bad yaml string"))?;

        // Convert YAML value into JSON value first, then allocate as Starlark value.
        let json_value = serde_json::to_value(yaml_value)
            .context(format_context!("Failed to convert yaml to json value"))?;
        let alloc_value = heap.alloc(json_value);

        Ok(alloc_value)
    }

    /// Converts a dictionary or Starlark value into a YAML-formatted string.
    ///
    /// This is the inverse of `string_to_dict`. It takes structured data
    /// and serializes it into a string, making it ready to be written
    /// to a file or sent over a network.
    ///
    /// ```python
    /// data = {"name": "Project Alpha", "version": 1}
    /// yaml_string = yaml.to_string(data)
    /// ```
    ///
    /// # Arguments
    /// * `value`: The dictionary or Starlark value to be serialized.
    ///
    /// # Returns
    /// * `str`: The YAML string representation of the input value.
    fn to_string(value: starlark::values::Value) -> anyhow::Result<String> {
        let yaml_string = serde_yaml::to_string(&value.to_json_value()?)
            .context(format_context!("Failed to convert dict to yaml string"))?;
        Ok(yaml_string)
    }

    /// Attempts to convert a YAML formatted string into a Starlark dictionary/value.
    /// Returns a Starlark None value if parsing fails instead of propagating an error.
    ///
    /// ```python
    /// raw_data = "id: 101\nstatus: active\n"
    /// result = yaml.try_string_to_dict(raw_data)
    /// if result != None:
    ///     print(result["status"])
    /// ```
    ///
    /// # Arguments
    /// * `content`: The YAML-formatted string to be converted.
    ///
    /// # Returns
    /// * `dict | None`: A dictionary representation of the YAML data, or None if parsing fails.
    fn try_string_to_dict<'v>(
        content: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        match serde_yaml::from_str::<serde_yaml::Value>(content) {
            Ok(yaml_value) => {
                let json_value = serde_json::to_value(yaml_value)
                    .context(format_context!("Failed to convert yaml to json value"))?;
                let alloc_value = heap.alloc(json_value);
                Ok(alloc_value)
            }
            Err(_) => Ok(heap.alloc(serde_json::Value::Null)),
        }
    }
}
