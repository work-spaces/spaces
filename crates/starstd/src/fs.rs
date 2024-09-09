use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::dict::{AllocDict, Dict};
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
        let mut dict = Dict::default();
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let alloc_dict = AllocDict(dict.iter());
        Ok(heap.alloc(alloc_dict))
    }
}
