#[macro_use]
extern crate starlark;

pub mod fs;

use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn print(content: starlark::values::Value) -> anyhow::Result<NoneType> {
        println!("{content:?}");
        Ok(NoneType)
    }
}
