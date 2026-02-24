use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;
use starlark::values::none::NoneType;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Writes a string to a file at the specified path relative to the workspace root.
    ///
    /// If the file already exists, its contents will be truncated (overwritten).
    /// If the file does not exist, it will be created.
    ///
    /// ```python
    /// fs.write_string_to_file(path = "config/settings.json", content = '{"theme": "dark"}')
    /// ```
    ///
    /// # Arguments
    /// * `path`: The destination path relative to the workspace root.
    /// * `content`: The string data to write into the file.
    fn write_string_to_file(
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

    /// Appends a string to the end of a file at the specified path.
    ///
    /// ```python
    /// fs.append_string_to_file(path = "log/output.txt", content = "New log entry\n")
    /// ```
    ///
    /// # Arguments
    /// * `path`: The destination path relative to the workspace root.
    /// * `content`: The string data to append to the file.
    fn append_string_to_file(
        #[starlark(require = named)] path: &str,
        #[starlark(require = named)] content: &str,
    ) -> anyhow::Result<NoneType> {
        use std::io::Write;

        let mut file = std::fs::OpenOptions::new()
            .append(true) // Open in append mode
            .create(true) // Create the file if it doesn't exist
            .open(path)
            .context(format_context!("Failed to open/create {path}"))?;

        file.write_all(content.as_bytes())
            .context(format_context!("Failed to write to file {}", path))?;

        Ok(NoneType)
    }

    /// Reads the contents of a file and returns it as a string.
    ///
    /// ```python
    /// content = fs.read_file_to_string("config/settings.json")
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to the file relative to the workspace root.
    ///
    /// # Returns
    /// * `str`: The contents of the file as a string.
    fn read_file_to_string(path: &str) -> anyhow::Result<String> {
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;
        Ok(content)
    }

    /// Returns true if the given path exists.
    ///
    /// ```python
    /// if fs.exists("config/settings.json"):
    ///     print("Settings file found")
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to check relative to the workspace root.
    ///
    /// # Returns
    /// * `bool`: True if the path exists, False otherwise.
    fn exists(path: &str) -> anyhow::Result<bool> {
        Ok(std::path::Path::new(path).exists())
    }

    /// Returns true if the given path is a file.
    ///
    /// ```python
    /// if fs.is_file("config/settings.json"):
    ///     print("Path is a file")
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to check relative to the workspace root.
    ///
    /// # Returns
    /// * `bool`: True if the path is a file, False otherwise.
    fn is_file(path: &str) -> anyhow::Result<bool> {
        Ok(std::path::Path::new(path).is_file())
    }

    /// Returns true if the given path is a directory.
    ///
    /// ```python
    /// if fs.is_directory("config"):
    ///     print("Path is a directory")
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to check relative to the workspace root.
    ///
    /// # Returns
    /// * `bool`: True if the path is a directory, False otherwise.
    fn is_directory(path: &str) -> anyhow::Result<bool> {
        Ok(std::path::Path::new(path).is_dir())
    }

    /// Returns true if the given path is a symbolic link.
    ///
    /// ```python
    /// if fs.is_symlink("bin/tool"):
    ///     print("Path is a symlink")
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to check relative to the workspace root.
    ///
    /// # Returns
    /// * `bool`: True if the path is a symbolic link, False otherwise.
    fn is_symlink(path: &str) -> anyhow::Result<bool> {
        Ok(std::path::Path::new(path).is_symlink())
    }

    /// Returns true if the given path is a text file.
    ///
    /// ```python
    /// if fs.is_text_file("output/result.dat"):
    ///     print("File contains text")
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to check relative to the workspace root.
    ///
    /// # Returns
    /// * `bool`: True if the path is a text file, False otherwise.
    fn is_text_file(path: &str) -> anyhow::Result<bool> {
        let file_path = std::path::Path::new(path);
        if !file_path.is_file() {
            return Ok(false);
        }

        let contents = std::fs::read_to_string(file_path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        // Check if the file is a text file by checking if it contains any non-UTF8 characters
        let is_text = contents.is_char_boundary(contents.len());

        Ok(is_text)
    }

    /// Reads a TOML file and returns its contents as a dictionary.
    ///
    /// ```python
    /// config = fs.read_toml_to_dict("Cargo.toml")
    /// print(config["package"]["name"])
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to the TOML file relative to the workspace root.
    ///
    /// # Returns
    /// * `dict`: The parsed TOML contents as a dictionary.
    fn read_toml_to_dict<'v>(
        path: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let toml_value: toml::Value = toml::from_str(&content)
            .context(format_context!("Failed to parse TOML file {}", path))?;

        let json_value = serde_json::to_value(toml_value)
            .context(format_context!("Failed to convert TOML to JSON {}", path))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);

        Ok(alloc_value)
    }

    /// Reads a YAML file and returns its contents as a dictionary.
    ///
    /// ```python
    /// config = fs.read_yaml_to_dict("config.yaml")
    /// print(config["settings"]["theme"])
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to the YAML file relative to the workspace root.
    ///
    /// # Returns
    /// * `dict`: The parsed YAML contents as a dictionary.
    fn read_yaml_to_dict<'v>(
        path: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&content)
            .context(format_context!("Failed to parse YAML file {}", path))?;

        let json_value = serde_json::to_value(&yaml_value)
            .context(format_context!("Failed to convert YAML to JSON {}", path))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);

        Ok(alloc_value)
    }

    /// Reads a JSON file and returns its contents as a dictionary.
    ///
    /// ```python
    /// data = fs.read_json_to_dict("package.json")
    /// print(data["name"])
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to the JSON file relative to the workspace root.
    ///
    /// # Returns
    /// * `dict`: The parsed JSON contents as a dictionary.
    fn read_json_to_dict<'v>(
        path: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let json_value: serde_json::Value = serde_json::from_str(&content)
            .context(format_context!("Failed to parse JSON file {}", path))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);
        Ok(alloc_value)
    }

    /// Reads the contents of a directory and returns a list of paths.
    ///
    /// ```python
    /// entries = fs.read_directory("src")
    /// for entry in entries:
    ///     print(entry)
    /// ```
    ///
    /// # Arguments
    /// * `path`: The path to the directory relative to the workspace root.
    ///
    /// # Returns
    /// * `list[str]`: A list of paths for each entry in the directory.
    fn read_directory(path: &str) -> anyhow::Result<Vec<String>> {
        let entries = std::fs::read_dir(path).context(format_context!(
            "Failed to read directory {} all paths must be relative to the workspace root",
            path
        ))?;

        let mut result = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let path = path
                .to_str()
                .context(format_context!("Failed to convert path to string"))?;
            result.push(path.to_string());
        }

        Ok(result)
    }
}
