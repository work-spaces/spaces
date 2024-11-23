use crate::{Arg, Function};
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use starlark::values::{Heap, Value};
use std::collections::HashMap;
use std::sync::RwLock;

struct State {
    exit_code: i32,
    args: Vec<String>,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(RwLock::new(State {
        exit_code: 0,
        args: Vec::new(),
    }));
    STATE.get()
}

pub fn set_args(script_args: Vec<String>) {
    let mut state = get_state().write().unwrap();
    state.args = script_args;
}

pub fn get_exit_code() -> i32 {
    let state = get_state().read().unwrap();
    state.exit_code
}

pub const FUNCTIONS: &[Function] = &[
        Function {
            name: "print",
            description: "Prints a string to the stdout. Only use in a script.",
            return_type: "None",
            args: &[Arg {
                name: "content",
                description: "str: string content to print.",
                dict: &[],
            }],
            example: None,

        },
        Function {
            name: "get_arg",
            description: "Gets the argument at the specified offset (an empty string is returned if the argument doesn't exist).",
            return_type: "str",
            args: &[Arg {
                name: "offset",
                description: "int: offset of the argument to get.",
                dict: &[],
            }],
            example: None,

        },
        Function {
            name: "get_args",
            description: "Gets the arguments as a dict with 'ordered' and 'named' keys. `ordered` is a list of arguments that do not contain =, `named` is a map of key value pairs separated by =.",
            return_type: "dict",
            args: &[Arg {
                name: "offset",
                description: "int: offset of the argument to get.",
                dict: &[],
            }],
            example: None,

        },
        Function {
            name: "set_exit_code",
            description: r#"Sets the exit code of the script. 
Use zero for success and non-zero for failure.
This doesn't exit the script."#,
            return_type: "none",
            args: &[Arg {
                name: "offset",
                description: "int: offset of the argument to get.",
                dict: &[],
            }],
            example: None,

        },
    ];

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn print(content: &str) -> anyhow::Result<NoneType> {
        println!("{content}");
        Ok(NoneType)
    }

    fn get_arg(offset: i32) -> anyhow::Result<String> {
        let state = get_state().read().unwrap();
        let offset = offset as usize;
        if offset >= state.args.len() {
            return Ok(String::new());
        }
        Ok(state.args[offset].clone())
    }

    #[allow(clippy::needless_lifetimes)]
    fn get_args<'v>(heap: &'v Heap) -> anyhow::Result<Value<'v>> {
        let mut result = serde_json::Value::Object(serde_json::Map::new());

        let mut list_args = Vec::new();
        let mut named_args = HashMap::new();

        let args = get_state().read().unwrap().args.clone();

        for arg in args.iter() {
            if arg.contains('=') {
                let parts: Vec<&str> = arg.split('=').collect();
                named_args.insert(parts[0].to_string(), parts[1].to_string());
            } else {
                list_args.push(arg.to_string());
            }
        }

        result["ordered"] = serde_json::to_value(list_args).unwrap();
        result["named"] = serde_json::to_value(named_args).unwrap();

        let alloc_value = heap.alloc(result);
        Ok(alloc_value)
    }

    fn set_exit_code(exit_code: i32) -> anyhow::Result<NoneType> {
        let mut state = get_state().write().unwrap();
        state.exit_code = exit_code;
        Ok(NoneType)
    }
}
