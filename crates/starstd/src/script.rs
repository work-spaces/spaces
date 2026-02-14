use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::none::NoneType;
use starlark::values::Value;
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

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Aborts execution immediately.
    ///
    /// This function terminates the script with a non-zero exit code (indicating
    /// failure) and prints the provided message to the standard error stream (stderr).
    ///
    /// ```python
    /// script.abort("Failed to do something")
    /// ```
    ///
    /// # Arguments
    /// * `message`: The error message to display upon termination.
    fn abort(message: &str) -> anyhow::Result<NoneType> {
        Err(anyhow::anyhow!(format!("Aborting: {message}")))
    }

    /// Outputs a string to the standard output (stdout).
    ///
    /// This is intended for use within script execution to provide feedback
    /// or data to the user.
    ///
    /// ```python
    /// script.print("Hello, world!")
    /// ```
    ///
    /// # Arguments
    /// * `content`: The message to print.
    fn print(content: &str) -> anyhow::Result<NoneType> {
        println!("{content}");
        Ok(NoneType)
    }

    /// Retrieves a specific command-line argument by its index.
    ///
    /// If no argument exists at the given offset, an empty string is returned.
    ///
    /// ```python
    /// first_arg = script.get_arg(0)
    /// print(f"First argument: {first_arg}")
    /// ```
    ///
    /// # Arguments
    /// * `offset`: The positional index of the argument (0-based).
    ///
    /// # Returns
    /// * `str`: The argument value or an empty string.
    fn get_arg(offset: i32) -> anyhow::Result<String> {
        let state = get_state().read().unwrap();
        let offset = offset as usize;
        if offset >= state.args.len() {
            return Ok(String::new());
        }
        Ok(state.args[offset].clone())
    }

    /// Parses command-line arguments into structured categories.
    ///
    /// This function separates "ordered" arguments (standalone values) from
    /// "named" arguments (key=value pairs).
    ///
    /// ```python
    /// args = script.get_args()
    /// for arg in args["ordered"]:
    ///     print(arg)
    /// ```
    ///
    /// # Returns
    /// * `dict`: A dictionary containing `ordered` (`list[str]`) and `named` (`dict`).
    #[allow(clippy::needless_lifetimes)]
    fn get_args<'v>(eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
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

    /// Sets the final exit code for the script without terminating it.
    ///
    /// Use 0 for success and non-zero for failure. The script will continue
    /// executing until it reaches the end or an `abort` call.
    ///
    /// ```python
    /// script.set_exit_code(1)
    /// ```
    ///
    /// # Arguments
    /// * `exit_code`: The integer exit code to be returned upon completion.
    fn set_exit_code(exit_code: i32) -> anyhow::Result<NoneType> {
        let mut state = get_state().write().unwrap();
        state.exit_code = exit_code;
        Ok(NoneType)
    }
}
