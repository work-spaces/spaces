use crate::singleton;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use starstd::{Arg, Function};
use starlark::values::{Heap, Value};
use std::sync::Arc;


pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "get_platform_name",
        description: "returns the name of the current platform: macos-aarch64|macos-x86_64|linux-x86_64|linux-aarch64|windows-x86_64|windows-aarch64",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "is_platform_windows",
        description: "returns true if platform is Windows",
        return_type: "bool",
        args: &[],
        example: None,
    },
    Function {
        name: "is_platform_macos",
        description: "returns true if platform is macos",
        return_type: "bool",
        args: &[],
        example: None,
    },
    Function {
        name: "is_platform_linux",
        description: "returns true if platform is linux",
        return_type: "bool",
        args: &[],
        example: None,
    },
    Function {
        name: "get_path_to_store",
        description: "returns the path to the spaces store (typically $HOME/.spaces/store)",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "get_supported_platforms",
        description: "returns a list of the supported platforms",
        return_type: "list[str]",
        args: &[],
        example: None,
    },
    Function {
        name: "get_cpu_count",
        description: "returns the number of CPUs on the current machine",
        return_type: "int",
        args: &[],
        example: None,
    },
    Function {
        name: "get_workspace_digest",
        description: "returns the digest of the workspace. This is only meaningful if the workspace is reproducible (which can't be known until after checkout)",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "is_ci",
        description: "returns true if `--ci` is passed on the command line",
        return_type: "int",
        args: &[],
        example: None,
    },
    Function {
        name: "get_log_divider_string",
        description: "returns a string representing the end of the log header",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "parse_log_file",
        description: "Parses the log file header from yaml and puts the lines into an array",
        return_type: "dict['header': dict, 'lines': list[str]]",
        args: &[            
            Arg {
                name: "path",
                description: "The path to the spaces log file",
                dict: &[],
            },
        ],
        example: None,
    },
    Function {
        name: "set_minimum_version",
        description: "sets the minimum version of spaces required to run the script",
        return_type: "int",
        args: &[
            Arg {
                name: "version",
                description: "the minimum version of spaces required to run the script",
                dict: &[],
            },
        ],
        example: None,
    },
    Function {
        name: "set_max_queue_count",
        description: "sets the maxiumum number of items to queue at one time",
        return_type: "int",
        args: &[
            Arg {
                name: "count",
                description: "the maximum number of items to queue at one time",
                dict: &[],
            },
        ],
        example: None,
    },
];

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn get_platform_name() -> anyhow::Result<String> {
        platform::Platform::get_platform()
            .map(|p| p.to_string())
            .ok_or(anyhow::anyhow!("Failed to get platform name"))
    }

    fn get_supported_platforms() -> anyhow::Result<Vec<String>> {
        Ok(platform::Platform::get_supported_platforms()
            .into_iter()
            .map(|p| p.to_string())
            .collect())
    }

    fn is_ci() -> anyhow::Result<bool> {
        Ok(singleton::get_is_ci())
    }

    fn is_platform_windows() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_windows())
    }

    fn is_platform_macos() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_macos())
    }

    fn is_platform_linux() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_linux())
    }

    fn is_platform_x86_64() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_x86_64())
    }

    fn is_platform_aarch64() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_aarch64())
    }

    fn abort(message: &str) -> anyhow::Result<NoneType> {
        Err(format_error!("Info Aborting: {}", message))
    }

    fn get_cpu_count() -> anyhow::Result<i64> {
        Ok(num_cpus::get() as i64)
    }

    fn parse_log_file<'v>(path: &str, heap: &'v Heap) -> anyhow::Result<Value<'v>> {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Log {
            header: printer::LogHeader,
            lines: Vec<Arc<str>>
        }

        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let mut header = String::new();
        let mut lines = Vec::new();
        let log_divider = printer::Printer::get_log_divider();
        let mut collect_header = true;
        for line in content.lines() {
            if line == log_divider.as_ref() {
                collect_header = false;
                continue;
            }
            if collect_header {
                header.push_str(line);
                header.push('\n');
            } else {
                lines.push(line.to_string().into());
            }
        }

        let log_header: printer::LogHeader = serde_yaml::from_str(&header)
            .context(format_context!("Failed to parse (yaml) Log Header file {}", path))?;

        let json_value = serde_json::to_value(&Log {
            header: log_header,
            lines: lines,
        }).context(format_context!("Internal Error: Failed to convert Log to JSON {}", path))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);
        Ok(alloc_value)

    }

    fn get_path_to_store() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.get_store_path().to_string())
    }

    fn get_path_to_spaces_tools() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.get_spaces_tools_path().to_string())
    }

    fn get_log_divider_string() -> anyhow::Result<String> {
        Ok(printer::Printer::get_log_divider().to_string())
    }

    fn set_minimum_version(version: &str) -> anyhow::Result<NoneType> {
        let current_version = env!("CARGO_PKG_VERSION");
        let version = version
            .parse::<semver::Version>()
            .context(format_context!("bad version format"))?;
        if version
            > current_version
                .parse::<semver::Version>()
                .context(format_context!(
                    "Internal Error: Failed to parse current version {current_version}"
                ))?
        {
            return Err(anyhow::anyhow!(
                "Minimum required `spaces` version is {}. `spaces` version is {current_version}",
                version.to_string(),
            ));
        }
        Ok(NoneType)
    }

    fn set_max_queue_count(count: i64) -> anyhow::Result<NoneType> {
        if count < 1 {
            return Err(anyhow::anyhow!("max_queue_count must be greater than 0"));
        }
        if count > 64 {
            return Err(anyhow::anyhow!("max_queue_count must be less than 65"));
        }
        singleton::set_max_queue_count(count);
        Ok(NoneType)
    }
}
