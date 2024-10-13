use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
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
        args: Vec::new()
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

    fn set_exit_code(exit_code: i32) -> anyhow::Result<NoneType> {
        let mut state = get_state().write().unwrap();
        state.exit_code = exit_code;
        Ok(NoneType)
    }

}
