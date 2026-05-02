use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;

/// Converts a `toml::Value` into a `serde_json::Value`.
///
/// * TOML datetime types are formatted as ISO 8601 strings (Starlark has no
///   native datetime type).
/// * TOML special float literals (`inf`, `-inf`, `nan`) are rejected with an
///   error because they have no JSON/Starlark representation.
fn toml_to_json(value: toml::Value) -> anyhow::Result<serde_json::Value> {
    match value {
        toml::Value::Boolean(b) => Ok(serde_json::Value::Bool(b)),
        toml::Value::Integer(i) => Ok(serde_json::Value::Number(i.into())),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .ok_or_else(|| {
                anyhow::anyhow!("TOML float {f} (inf/nan) has no Starlark representation")
            }),
        toml::Value::String(s) => Ok(serde_json::Value::String(s)),
        toml::Value::Datetime(dt) => Ok(serde_json::Value::String(format!("{dt}"))),
        toml::Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(toml_to_json(item)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        toml::Value::Table(table) => {
            let mut map = serde_json::Map::new();
            for (k, v) in table {
                map.insert(k, toml_to_json(v)?);
            }
            Ok(serde_json::Value::Object(map))
        }
    }
}

/// Converts a `serde_json::Value` into a `toml::Value`.
///
/// JSON `null` is rejected because TOML has no null type. Remove or replace
/// null fields before encoding to TOML.
fn json_to_toml(value: serde_json::Value) -> anyhow::Result<toml::Value> {
    match value {
        serde_json::Value::Null => Err(anyhow::anyhow!(
            "TOML does not support null values; \
             remove null fields before encoding to TOML"
        )),
        serde_json::Value::Bool(b) => Ok(toml::Value::Boolean(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Err(anyhow::anyhow!(
                    "Cannot represent JSON number {n} as a TOML value"
                ))
            }
        }
        serde_json::Value::String(s) => Ok(toml::Value::String(s)),
        serde_json::Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(json_to_toml(item)?);
            }
            Ok(toml::Value::Array(out))
        }
        serde_json::Value::Object(map) => {
            let mut table = toml::map::Map::new();
            for (k, v) in map {
                table.insert(k, json_to_toml(v)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

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
    ///
    /// # Notes
    /// * TOML datetime values (`1979-05-27T07:32:00Z`, `1979-05-27`, `07:32:00`)
    ///   are returned as ISO 8601 strings because Starlark has no datetime type.
    /// * TOML special float literals (`inf`, `-inf`, `nan`) are not supported
    ///   and will cause an error.
    fn string_to_dict<'v>(
        content: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        if crate::is_lsp_mode() {
            // In LSP mode behave like try_string_to_dict: attempt the parse and
            // return the real value on success so that the LSP gets accurate type
            // information.  Fall back to an empty dict on any failure instead of
            // propagating an error.
            return Ok(match toml::from_str::<toml::Value>(content) {
                Ok(toml_value) => match toml_to_json(toml_value) {
                    Ok(json_value) => heap.alloc(json_value),
                    Err(_) => heap.alloc(serde_json::json!({})),
                },
                Err(_) => heap.alloc(serde_json::json!({})),
            });
        }
        let toml_value: toml::Value =
            toml::from_str(content).context(format_context!("bad toml string"))?;
        let json_value = toml_to_json(toml_value)
            .context(format_context!("Failed to convert TOML to Starlark"))?;
        Ok(heap.alloc(json_value))
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
        let json_value = value
            .to_json_value()
            .context(format_context!("Failed to convert Starlark value to JSON"))?;
        let toml_value =
            json_to_toml(json_value).context(format_context!("Failed to convert value to TOML"))?;
        toml::to_string(&toml_value).context(format_context!("Failed to serialize TOML string"))
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
        let json_value = value
            .to_json_value()
            .context(format_context!("Failed to convert Starlark value to JSON"))?;
        let toml_value =
            json_to_toml(json_value).context(format_context!("Failed to convert value to TOML"))?;
        toml::to_string_pretty(&toml_value)
            .context(format_context!("Failed to serialize pretty TOML string"))
    }

    /// Returns `True` if the given string is valid TOML, `False` otherwise.
    ///
    /// ```python
    /// if toml.is_string_toml('key = "value"'):
    ///     print("Valid TOML")
    /// ```
    ///
    /// # Arguments
    /// * `value`: The string to check.
    ///
    /// # Returns
    /// * `bool`: `True` if the string parses as valid TOML, `False` otherwise.
    fn is_string_toml(value: &str) -> anyhow::Result<bool> {
        Ok(toml::from_str::<toml::Value>(value).is_ok())
    }

    /// Attempts to convert a TOML formatted string into a Starlark dictionary/value.
    /// Returns the `default` value (defaulting to `None`) if parsing fails instead
    /// of propagating an error.
    ///
    /// On success, returns the parsed Starlark value. On failure, returns the
    /// value supplied to `default` (which itself defaults to `None`).
    ///
    /// A named `default` parameter lets callers supply a sentinel distinguishable
    /// from a successful but empty parse result:
    ///
    /// ```python
    /// MISSING = "PARSE_FAILED"
    /// result = toml.try_string_to_dict(raw, default = MISSING)
    /// if result == MISSING:
    ///     print("input was not valid TOML")
    /// else:
    ///     print(result["key"])
    /// ```
    ///
    /// When called without `default` the behaviour is: `None` is returned on
    /// parse failure.
    ///
    /// ```python
    /// raw_data = 'id = 101\nstatus = "active"'
    /// result = toml.try_string_to_dict(raw_data)
    /// if result != None:
    ///     print(result["status"])
    /// else:
    ///     print("Failed to parse TOML")
    /// ```
    ///
    /// # Arguments
    /// * `content`: The TOML-formatted string to be converted.
    /// * `default` *(named, optional)*: Value to return when parsing fails.
    ///   Defaults to `None`.
    ///
    /// # Returns
    /// * `dict | <default>`: A Starlark value representing the parsed TOML, or
    ///   the `default` value if parsing fails.
    ///
    /// # Notes
    /// * TOML datetime values are returned as ISO 8601 strings.
    /// * TOML special floats (`inf`, `-inf`, `nan`) cause an error even in this
    ///   try variant because the failure occurs post-parse during conversion.
    fn try_string_to_dict<'v>(
        content: &str,
        #[starlark(require = named)] default: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        match toml::from_str::<toml::Value>(content) {
            Ok(toml_value) => {
                let json_value = toml_to_json(toml_value)
                    .context(format_context!("Failed to convert TOML to Starlark"))?;
                Ok(heap.alloc(json_value))
            }
            Err(_) => Ok(default.unwrap_or_else(Value::new_none)),
        }
    }
}
