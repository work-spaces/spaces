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

#[derive(Debug, Clone)]
pub struct Arg {
    pub name: &'static str,
    pub description: &'static str,
    pub dict: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone)]
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
            ("platforms", "optional list of platforms to run on. If not provided, rule will run on all platforms. See above for details"),
            ("type", "Checkout|Optional (Default)|Setup|Run: see above for details"),
            ("help", "Optional help text show with `spaces inspect`"),
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
