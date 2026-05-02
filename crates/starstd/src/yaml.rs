use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;

/// Converts a `serde_json::Value` into a `serde_yaml::Value`.
///
/// This explicit conversion is necessary because calling
/// `serde_yaml::to_string(&serde_json_value)` directly causes
/// `serde_json::Number` to serialize using its internal
/// `$serde_json::private::Number` envelope, which `serde_yaml` does not
/// understand and renders as a nested mapping instead of a scalar number.
///
/// By walking the JSON value tree and constructing native `serde_yaml` types,
/// we avoid that serialization mismatch entirely.
///
/// * JSON `null`  → YAML `Null`
/// * JSON `bool`  → YAML `Bool`
/// * JSON integer → YAML `Number` (integer)
/// * JSON float   → YAML `Number` (float)
/// * JSON string  → YAML `String`
/// * JSON array   → YAML `Sequence`
/// * JSON object  → YAML `Mapping` (string keys, insertion order preserved)
fn json_to_yaml(value: serde_json::Value) -> anyhow::Result<serde_yaml::Value> {
    match value {
        serde_json::Value::Null => Ok(serde_yaml::Value::Null),
        serde_json::Value::Bool(b) => Ok(serde_yaml::Value::Bool(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(serde_yaml::Value::Number(i.into()))
            } else if let Some(u) = n.as_u64() {
                Ok(serde_yaml::Value::Number(u.into()))
            } else if let Some(f) = n.as_f64() {
                Ok(serde_yaml::Value::Number(f.into()))
            } else {
                Err(anyhow::anyhow!(
                    "Cannot represent JSON number {} as a YAML value",
                    n
                ))
            }
        }
        serde_json::Value::String(s) => Ok(serde_yaml::Value::String(s)),
        serde_json::Value::Array(arr) => {
            let mut seq = serde_yaml::Sequence::new();
            for item in arr {
                seq.push(json_to_yaml(item)?);
            }
            Ok(serde_yaml::Value::Sequence(seq))
        }
        serde_json::Value::Object(map) => {
            let mut yaml_map = serde_yaml::Mapping::new();
            for (k, v) in map {
                yaml_map.insert(serde_yaml::Value::String(k), json_to_yaml(v)?);
            }
            Ok(serde_yaml::Value::Mapping(yaml_map))
        }
    }
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Converts a YAML formatted string into a Starlark dictionary/value.
    ///
    /// The YAML is parsed with `serde_yaml`, which:
    /// * **Multi-document input is not supported** — if the input contains
    ///   multiple `---`-separated documents, `serde_yaml` 0.9 returns a parse
    ///   error.  Split multi-document streams into individual document strings
    ///   before calling this function.  A single document that begins with a
    ///   leading `---` marker is accepted.
    /// * **Resolves anchors and aliases transparently** — `&anchor` / `*alias`
    ///   syntax is expanded during parsing.  Circular references are rejected
    ///   with an error.
    /// * **The YAML 1.1 merge key `<<:` is NOT supported** — `serde_yaml` 0.9
    ///   targets YAML 1.2, which does not include merge keys.  `<<` is treated
    ///   as a plain string key.  Perform merges manually with `yaml_merge()`.
    /// * **Does not evaluate arbitrary tags** — unknown YAML tags (e.g.
    ///   `!!python/object`) are rejected; the loader is safe.
    /// * **Round-trip lossiness** — comments, key insertion order, and quoting
    ///   style are **not** preserved.  `inf`, `-inf`, and `.nan` float literals
    ///   are valid YAML but have no JSON/Starlark representation and will
    ///   produce an error.
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
        if crate::is_lsp_mode() {
            // In LSP mode behave like try_string_to_dict: attempt the parse and
            // return the real value on success so that the LSP gets accurate type
            // information.  Fall back to an empty dict on any failure instead of
            // propagating an error.
            return Ok(match serde_yaml::from_str::<serde_yaml::Value>(content) {
                Ok(yaml_value) => match serde_json::to_value(yaml_value) {
                    Ok(json_value) => heap.alloc(json_value),
                    Err(_) => heap.alloc(serde_json::json!({})),
                },
                Err(_) => heap.alloc(serde_json::json!({})),
            });
        }
        let yaml_value: serde_yaml::Value =
            serde_yaml::from_str(content).context(format_context!("bad yaml string"))?;

        // Convert YAML value into JSON value first, then allocate as Starlark value.
        // serde_yaml::Value::Number has a proper Serialize impl (dispatches to
        // serialize_u64 / serialize_i64 / serialize_f64), so serde_json::to_value
        // handles it correctly in this direction.
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
    /// **Round-trip notes**: The serialized YAML will not contain comments,
    /// anchors/aliases, or the original quoting style.  Key ordering follows
    /// the Starlark dict iteration order (insertion order), which may differ
    /// from the original source if the value was parsed from YAML.
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
        // We must NOT call serde_yaml::to_string(&json_value) directly.
        // serde_json::Number serializes itself with an internal
        // `$serde_json::private::Number` wrapper key that serde_yaml treats as
        // a regular mapping, producing garbage output like:
        //   count:
        //     $serde_json::private::Number: '42'
        // Instead, convert the JSON value into a native serde_yaml::Value first.
        let json_value = value
            .to_json_value()
            .context(format_context!("Failed to convert Starlark value to JSON"))?;
        let yaml_value =
            json_to_yaml(json_value).context(format_context!("Failed to convert JSON to YAML"))?;
        let yaml_string = serde_yaml::to_string(&yaml_value)
            .context(format_context!("Failed to serialize YAML string"))?;
        Ok(yaml_string)
    }

    /// Returns `True` if the given string is valid YAML (single-document),
    /// `False` otherwise.
    ///
    /// Only the first document is validated.  The function never raises an
    /// error; it always returns a boolean.
    ///
    /// ```python
    /// if yaml.is_string_yaml("key: value"):
    ///     print("Valid YAML")
    /// ```
    ///
    /// # Arguments
    /// * `value`: The string to check.
    ///
    /// # Returns
    /// * `bool`: `True` if the string parses as valid YAML, `False` otherwise.
    fn is_string_yaml(value: &str) -> anyhow::Result<bool> {
        Ok(serde_yaml::from_str::<serde_yaml::Value>(value).is_ok())
    }

    /// Attempts to convert a YAML formatted string into a Starlark dictionary/value.
    ///
    /// On success, returns the parsed Starlark value.  On failure, returns the
    /// value supplied to `default` (which itself defaults to `None`).
    ///
    /// A named `default` parameter lets callers supply a sentinel that is
    /// distinguishable from a successfully decoded YAML `null`:
    ///
    /// ```python
    /// MISSING = "PARSE_FAILED"
    /// result = yaml.try_string_to_dict(raw, default = MISSING)
    /// if result == MISSING:
    ///     print("input was not valid YAML")
    /// elif result == None:
    ///     print("input was the YAML null literal")
    /// else:
    ///     print(result["key"])
    /// ```
    ///
    /// When called without `default` the behaviour is: `None` is returned on
    /// parse failure.
    ///
    /// ```python
    /// raw_data = "id: 101\nstatus: active\n"
    /// result = yaml.try_string_to_dict(raw_data)
    /// if result != None:
    ///     print(result["status"])
    /// else:
    ///     print("Failed to parse YAML")
    /// ```
    ///
    /// # Arguments
    /// * `content`: The YAML-formatted string to be converted.
    /// * `default` *(named, optional)*: Value to return when parsing fails.
    ///   Defaults to `None`.
    ///
    /// # Returns
    /// * `dict | <default>`: A Starlark value representing the parsed YAML, or
    ///   the `default` value if parsing fails.
    ///
    /// # Notes
    /// * Multi-document input (multiple `---`-separated documents) causes a
    ///   parse error; `default` is returned in that case.
    /// * `inf`, `-inf`, and `.nan` floats cause a post-parse conversion error
    ///   even in this try variant.
    fn try_string_to_dict<'v>(
        content: &str,
        #[starlark(require = named)] default: Option<Value<'v>>,
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
            Err(_) => Ok(default.unwrap_or_else(Value::new_none)),
        }
    }
}
