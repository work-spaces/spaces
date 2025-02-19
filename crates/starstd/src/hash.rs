use crate::{Arg, Function};
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;

pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "compute_sha256_from_file",
        description:
            "Computes the sha256 checksum for the contents of a file and returns the digest as a string.",
        return_type: "String",
        args: &[
            Arg {
                name: "file_path",
                description: "path to the file",
                dict: &[],
            },
        ],
        example: None,
    },
    Function {
        name: "compute_sha256_from_string",
        description:
            "Computes the sha256 checksum of the given string.",
        return_type: "String",
        args: &[
            Arg {
                name: "input",
                description: "input string to hash",
                dict: &[],
            },
        ],
        example: None,
    },
];

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn compute_sha256_from_file(file_path: &str) -> anyhow::Result<String> {
        let file_contents = std::fs::read(file_path).context(format_context!("{file_path}"))?;
        let digest = sha256::digest(file_contents);
        Ok(digest)
    }

    fn compute_sha256_from_string(input: &str) -> anyhow::Result<String> {
        Ok(sha256::digest(input.as_bytes()))
    }
}
