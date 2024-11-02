use crate::{Arg, Function};
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;
use starlark::values::{Heap, Value};
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exec {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub working_directory: Option<String>,
    pub stdin: Option<String>,
}

pub const FUNCTIONS: &[Function] = &[Function {
    name: "exec",
    description: "Executes a process",
    return_type: "dict # with members `status`, `stdout`, and `stderr`",
    args: &[
        Arg {
            name: "exec",
            description: "dict with members",
            dict: &[
                ("command", "name of the command to execute"),
                ("args", "optional list of arguments"),
                ("env", "optional dict of environment variables"),
                (
                    "working_directory",
                    "optional working directory (default is the workspace)",
                ),
                ("stdin", "optional string to pipe to the process stdin"),
            ],
        },
        Arg {
            name: "content",
            description: "contents to write",
            dict: &[],
        },
    ],
    example: None,
}];

// This defines the functions that are visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn exec<'v>(exec: starlark::values::Value, heap: &'v Heap) -> anyhow::Result<Value<'v>> {
        let exec: Exec = serde_json::from_value(exec.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        let exec_stdin = exec.stdin;

        let mut command = Command::new(exec.command);
        for arg in exec.args.unwrap_or_default() {
            command.arg(arg);
        }

        for (name, value) in exec.env.unwrap_or_default() {
            command.env(name, value);
        }

        if exec_stdin.is_some() {
            // send stdin to the process on standard input
            command.stdin(std::process::Stdio::piped());
        }

        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        if let Some(working_directory) = exec.working_directory {
            command.current_dir(working_directory);
        }

        let child_result = command.spawn();

        if let Ok(mut child) = child_result {
            if let Some(stdin) = exec_stdin {
                use std::io::Write;
                let child_stdin = child.stdin.as_mut().unwrap();
                child_stdin
                    .write_all(stdin.as_bytes())
                    .context(format_context!("Failed to write to stdin"))?;
            }

            let output_result = child.wait_with_output();
            let (status, stdout, stderr) = match output_result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    (
                        output.status.code().unwrap_or(1),
                        stdout.to_string(),
                        stderr.to_string(),
                    )
                }
                Err(e) => (1, String::new(), e.to_string()),
            };

            let mut result_map = serde_json::Map::new();
            result_map.insert(
                "status".to_string(),
                serde_json::Value::Number(status.into()),
            );
            result_map.insert(
                "stdout".to_string(),
                serde_json::Value::String(stdout.to_string()),
            );
            result_map.insert(
                "stderr".to_string(),
                serde_json::Value::String(stderr.to_string()),
            );
            Ok(heap.alloc(serde_json::Value::Object(result_map)))
        } else {
            Err(child_result.unwrap_err()).context("Failed to spawn child process")
        }
    }
}
