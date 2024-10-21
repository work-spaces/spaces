#[macro_use]
extern crate starlark;

pub mod fs;
pub mod process;
pub mod script;

use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;


pub struct Arg {
    pub name: &'static str,
    pub description: &'static str,
    pub dict: &'static [(&'static str, &'static str)],
}

pub struct Function {
    pub name: &'static str,
    pub description: &'static str,
    pub return_type: &'static str,
    pub args: &'static [Arg],
    pub example: Option<&'static str>,
}

pub const fn get_rule_argument() -> Arg {
    Arg {
        name: "rule",
        description: "dict",
        dict: &[
            ("name", "rule name as string"),
            ("deps", "list of dependencies"),
            ("type", "Setup|Run (default)|Optional"),
            ("help", "Optional help text show with `spaces evaluate`"),
        ],
    }
}


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
