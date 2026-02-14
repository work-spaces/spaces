#[macro_use]
extern crate starlark;

pub mod fs;
pub mod hash;
pub mod json;
pub mod process;
pub mod script;
pub mod time;

use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn print(content: &str) -> anyhow::Result<NoneType> {
        println!("{content}");
        Ok(NoneType)
    }

    fn debug(content: starlark::values::Value) -> anyhow::Result<NoneType> {
        println!("{content:?}");
        Ok(NoneType)
    }
}
