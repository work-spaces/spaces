use crate::{Arg, Function};
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;

pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "now",
        description: "Gets the current time in seconds since the Unix epoch.",
        return_type: "float",
        args: &[],
        example: None,
    },
    Function {
        name: "sleep",
        description: "sleeps for the given number of seconds",
        return_type: "None",
        args: &[Arg {
            name: "seconds",
            description: "Number of seconds to sleep",
            dict: &[],
        }],
        example: None,
    },
];

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn now() -> anyhow::Result<(u64, u32)> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context(format_context!("Failed to get current time"))?;
        Ok((current_time.as_secs(), current_time.subsec_nanos()))
    }

    fn sleep(nanoseconds: u64) -> anyhow::Result<NoneType> {
        std::thread::sleep(std::time::Duration::from_nanos(nanoseconds));
        Ok(NoneType)
    }
}
