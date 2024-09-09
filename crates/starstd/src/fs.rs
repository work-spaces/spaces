use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use starlark::values::{Heap, Value};

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn create_file(
        #[starlark(require = named)] path: &str,
        #[starlark(require = named)] content: &str,
    ) -> anyhow::Result<NoneType> {
        use std::io::Write;
        let mut file = std::fs::File::create(path).context(format_context!(
            "Failed to create file {} all paths must be relative to the workspace root",
            path
        ))?;

        file.write_all(content.as_bytes())
            .context(format_context!("Failed to write to file {}", path))?;

        Ok(NoneType)
    }

    fn read_file(path: &str) -> anyhow::Result<String> {
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;
        Ok(content)
    }

    fn read_toml<'v>(path: &str, heap: &'v Heap) -> anyhow::Result<Value<'v>> {
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let toml_value: toml::Value = toml::from_str(&content).context(format_context!(
            "Failed to parse TOML file {}",
            path
        ))?;

        let json_value = serde_json::to_value(&toml_value).context(format_context!(
            "Failed to convert TOML to JSON {}",
            path
        ))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);

        Ok(alloc_value)
    }

    fn read_yaml<'v>(path: &str, heap: &'v Heap) -> anyhow::Result<Value<'v>> {
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&content).context(format_context!(
            "Failed to parse TOML file {}",
            path
        ))?;

        let json_value = serde_json::to_value(&yaml_value).context(format_context!(
            "Failed to convert TOML to JSON {}",
            path
        ))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);

        Ok(alloc_value)
    }

    fn read_json<'v>(path: &str, heap: &'v Heap) -> anyhow::Result<Value<'v>> {
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let json_value: serde_json::Value = serde_json::from_str(&content).context(format_context!(
            "Failed to parse JSON file {}",
            path
        ))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);
        Ok(alloc_value)
    }
}
