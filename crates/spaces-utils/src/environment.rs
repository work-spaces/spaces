use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use printer::markdown;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

const ASSIGN_VARIABLES_DESCRIPTION: &str = r#"Values set in checkout rules. Part of the rule digest unless filtered. Overridden by `--env=NAME=VALUE`."#;

const AUTOMATIC_VARIABLES_DESCRIPTION: &str = r#"Workspace-specific values assigned by spaces during checkout (e.g. workspace path, digest). Other variables referencing these values use placeholders that are resolved at run time. This keeps rule digests stable across workspaces and enables cross-workspace rule caching."#;

const LIST_VARIABLES_DESCRIPTION: &str = r#"`Prepend` and `Append` variables assign multiple values to the same variable (e.g. `PATH`)."#;

const INHERIT_VARIABLES_DESCRIPTION: &str = r#"Values inherited from the calling environment. Not part of the workspace digest and must not affect reproducibility. Use for secrets and developer preferences.

- `default_value`: Fallback if the variable is not in the calling environment.
- `is_required`: Fail if the variable is missing.
- `is_secret`: Redact the value in logs.
- `is_save`: Persist the inherited value at checkout."#;

const SCRIPT_VARIABLES_DESCRIPTION: &str = r#"Values produced by running a shell script at checkout. The script runs with a clean environment.

- `default_value`: Fallback if the script fails.
- `shell`: Shell to use (default `/bin/sh`).
- `env`: Environment variables passed to the script.
- `is_required`: Fail if the script produces no output.
- `is_secret`: Redact the value in logs."#;

const ASSIGN_FROM_COMMAND_LINE_DESCRIPTION: &str = r#"Values set via `--env=NAME=VALUE`.

At checkout: included in the workspace digest and persisted. At run: NOT included in the digest; triggers re-evaluation of all modules. Always overrides existing workspace values."#;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum EnvBool {
    #[default]
    No,
    Yes,
}

impl From<bool> for EnvBool {
    fn from(value: bool) -> Self {
        if value { EnvBool::Yes } else { EnvBool::No }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AssignValue {
    pub value: Arc<str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AppendPrependValue {
    pub value: Arc<str>,
    pub separator: Arc<str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct InheritValue {
    /// The value of the inherited variable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Arc<str>>,
    /// Default value to use if the variable cannot be inherited
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assign_as_default: Option<Arc<str>>,
    /// if Yes, an error will occur if the variable is not available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_required: Option<EnvBool>,
    /// if Yes, redact the value in the logs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_secret: Option<EnvBool>,
    /// if Yes, save the variable value at checkout. If No, the value cannot impact reproducibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_save_at_checkout: Option<EnvBool>,
}

impl InheritValue {
    fn get_value(&self, name: &str) -> anyhow::Result<Option<Arc<str>>> {
        if let Some(value) = self.value.clone() {
            return Ok(Some(value));
        }

        let env_result = std::env::var(name);
        match env_result {
            Ok(env_value) => Ok(Some(env_value.into())),
            Err(e) => {
                if let (Some(EnvBool::Yes), None) =
                    (self.is_required, self.assign_as_default.clone())
                {
                    Err(format_error!(
                        "{name} is required to be inherited from calling env but is not available {e}",
                    ))
                } else if let Some(env_value) = self.assign_as_default.clone() {
                    Ok(Some(env_value.clone()))
                } else {
                    Ok(None)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ScriptValue {
    /// The script to evaluate to get the value of the variable.
    pub script: Arc<str>,
    /// Environment variables to pass to script evaluation (no other variables will be passed)
    pub env: HashMap<Arc<str>, Arc<str>>,
    /// The value of the script variable populated using the script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Arc<str>>,
    /// The shell to use to evaluate the script. The default is /bin/sh
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<Arc<str>>,
    /// Default value to use if the variable cannot be inherited
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assign_as_default: Option<Arc<str>>,
    /// if Yes, an error will occur if the variable is not available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_required: Option<EnvBool>,
    /// if Yes, redact the value in the logs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_secret: Option<EnvBool>,
    /// if Yes, save the variable value at checkout. If No, the value cannot impact reproducibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_save_at_checkout: Option<EnvBool>,
}

impl ScriptValue {
    fn get_value(&self, name: &str) -> anyhow::Result<Option<Arc<str>>> {
        if let Some(value) = self.value.clone() {
            return Ok(Some(value));
        }

        let command = self.shell.clone().unwrap_or_else(|| Arc::from("/bin/sh"));

        let mut script_builder = std::process::Command::new(command.as_ref());
        script_builder
            .env_clear()
            .arg("-c")
            .arg(self.script.as_ref());

        for (key, value) in self.env.iter() {
            script_builder.env(key.as_ref(), value.as_ref());
        }

        let script_result = script_builder.output();

        match script_result {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if stdout.is_empty() {
                    if let (Some(EnvBool::Yes), None) =
                        (self.is_required, self.assign_as_default.clone())
                    {
                        Err(format_error!(
                            "{name} script produced no output and is required",
                        ))
                    } else if let Some(default_value) = self.assign_as_default.clone() {
                        Ok(Some(default_value))
                    } else {
                        Ok(None)
                    }
                } else {
                    Ok(Some(stdout.into()))
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if let (Some(EnvBool::Yes), None) =
                    (self.is_required, self.assign_as_default.clone())
                {
                    Err(format_error!(
                        "{name} script failed with status {} and is required: {stderr}",
                        output.status,
                    ))
                } else if let Some(default_value) = self.assign_as_default.clone() {
                    Ok(Some(default_value))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                if let (Some(EnvBool::Yes), None) =
                    (self.is_required, self.assign_as_default.clone())
                {
                    Err(format_error!(
                        "{name} script failed to execute and is required: {e}",
                    ))
                } else if let Some(default_value) = self.assign_as_default.clone() {
                    Ok(Some(default_value))
                } else {
                    Ok(None)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub enum Value {
    #[default]
    None,
    /// Sets the value of the environment variable.
    /// May be overriden by a command line argument.
    /// Affects reproducibility.
    Assign(AssignValue),
    /// Prepend or create a value
    Append(AppendPrependValue),
    /// Prepend or create a value
    Prepend(AppendPrependValue),
    /// Inherit from the calling environment at checkout
    /// Must not affect reproducibility.
    Inherit(InheritValue),
    /// Execute a script in the calling environment.
    /// Must not affect reproducibility.
    Script(ScriptValue),
    /// Inherit at both checkout/run time, if available, and redact
    /// Must not affect reproducibility.
    AssignFromArg(Arc<str>),
    /// Automatic variables are assigned by spaces on-demand.
    /// They have no user-supplied value; the value is populated
    /// automatically during workspace operations.
    Automatic,
}

const AUTOMATIC_PLACEHOLDER_PREFIX: &str = "$AUTO";

impl Value {
    pub fn get_automatic_placeholder(name: &str) -> String {
        format!("{AUTOMATIC_PLACEHOLDER_PREFIX}{{{name}}}")
    }

    pub fn replace_with_automatic_placeholders(
        value: &str,
        auto_vars: &HashMap<&str, Arc<str>>,
    ) -> String {
        let mut result = value.to_string();
        for (auto_name, auto_value) in auto_vars.iter() {
            if !auto_value.is_empty() {
                let placeholder = Self::get_automatic_placeholder(auto_name);
                result = result.replace(auto_value.as_ref(), &placeholder);
            }
        }
        result
    }
}

/// Represents an update to an environment.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Any {
    /// Name of the environment variable to update.
    pub name: Arc<str>,
    /// Value to set for the environment variable.
    pub value: Value,
    /// source of the variable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Arc<str>>,
    /// Description of the environment variable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<Arc<str>>,
}

impl Any {
    pub fn new_assign_from_arg(name: Arc<str>, value: Arc<str>) -> Self {
        Self {
            name,
            value: Value::AssignFromArg(value),
            source: Some("assigned from command line arguments".into()),
            ..Default::default()
        }
    }

    pub fn new_set_value(name: Arc<str>, value: Arc<str>) -> Self {
        Self {
            name,
            value: Value::Assign(AssignValue { value }),
            ..Default::default()
        }
    }

    pub fn new_inherit_value(name: Arc<str>) -> Self {
        Self {
            name,
            value: Value::Inherit(InheritValue {
                is_required: Some(EnvBool::Yes),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    pub fn new_try_inherit_value(name: Arc<str>) -> Self {
        Self {
            name,
            value: Value::Inherit(InheritValue::default()),
            ..Default::default()
        }
    }

    pub fn new_secret_value(name: Arc<str>) -> Self {
        Self {
            name,
            value: Value::Inherit(InheritValue {
                is_required: Some(EnvBool::Yes),
                is_secret: Some(EnvBool::Yes),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    pub fn new_path_value(path: Arc<str>) -> Self {
        Self {
            name: "PATH".into(),
            value: Value::Prepend(AppendPrependValue {
                value: path,
                separator: ":".into(),
            }),
            ..Default::default()
        }
    }

    pub fn new_system_path_value(system_path: Arc<str>) -> Self {
        Self {
            name: "PATH".into(),
            value: Value::Append(AppendPrependValue {
                value: system_path,
                separator: ":".into(),
            }),
            ..Default::default()
        }
    }

    pub fn new_automatic(name: Arc<str>, help: Option<Arc<str>>) -> Self {
        Self {
            name,
            value: Value::Automatic,
            source: Some("automatically assigned by spaces".into()),
            help,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnyEnvironment {
    pub vars: Vec<Any>,
}

impl TryFrom<serde_json::Value> for AnyEnvironment {
    type Error = anyhow::Error;
    fn try_from(value: serde_json::Value) -> anyhow::Result<Self> {
        let env_result = serde_json::from_value::<Environment>(value.clone());
        let any_env = match env_result {
            Ok(env) => AnyEnvironment::from(&env),
            Err(_) => serde_json::from_value::<AnyEnvironment>(value)
                .context(format_context!("Failed to parse AnyEnvironment from value"))?,
        };
        Ok(any_env)
    }
}

impl AnyEnvironment {
    pub fn new() -> Self {
        Self { vars: Vec::new() }
    }

    pub fn is_env_var_set(&self, name: &str) -> bool {
        let vars = self.get_vars().unwrap_or_default();
        vars.contains_key(name)
    }

    pub fn is_env_var_set_to(&self, name: &str, value: &str) -> bool {
        let vars = self.get_vars().unwrap_or_default();
        if let Some(env_value) = vars.get(name) {
            env_value.as_ref() == value
        } else {
            false
        }
    }

    pub fn insert_or_update(&mut self, any: Any) {
        if let Some(index) = self.vars.iter().position(|v| v.name == any.name) {
            if matches!(self.vars[index].value, Value::Prepend(_))
                || matches!(self.vars[index].value, Value::Append(_))
                || matches!(self.vars[index].value, Value::AssignFromArg(_))
                || matches!(any.value, Value::AssignFromArg(_))
            {
                self.vars.push(any);
            } else {
                self.vars[index] = any;
            }
        } else {
            self.vars.push(any);
        }
    }

    pub fn insert_assign_from_args(&mut self, args_env: &HashMap<Arc<str>, Arc<str>>) {
        for (name, value) in args_env {
            self.insert_or_update(Any {
                name: name.clone(),
                value: Value::AssignFromArg(value.clone()),
                help: Some("Inserting by user from the command line".into()),
                ..Default::default()
            });
        }
    }

    pub fn replace_values_with_automatic_placeholders(
        &mut self,
        auto_vars: &HashMap<&str, Arc<str>>,
    ) {
        for any in self.vars.iter_mut() {
            match &mut any.value {
                Value::Assign(assign_value) => {
                    assign_value.value =
                        Value::replace_with_automatic_placeholders(&assign_value.value, auto_vars)
                            .into();
                }
                Value::Append(append_prepend) | Value::Prepend(append_prepend) => {
                    append_prepend.value = Value::replace_with_automatic_placeholders(
                        &append_prepend.value,
                        auto_vars,
                    )
                    .into();
                }
                Value::AssignFromArg(value) => {
                    *value = Value::replace_with_automatic_placeholders(value, auto_vars).into();
                }
                Value::None | Value::Automatic | Value::Inherit(_) | Value::Script(_) => {}
            }
        }
    }

    pub fn append(&mut self, other: Self) {
        for any in other.vars {
            self.insert_or_update(any);
        }
    }

    pub fn retain_vars_from_args(&mut self) {
        self.vars
            .retain(|var| matches!(var.value, Value::AssignFromArg(_)));
    }

    /// This can be called on an AnyEnvironment passed in from a checkout rule
    /// to apply the same source module to all variables.
    pub fn populate_source_for_all(&mut self, source: Option<Arc<str>>) {
        for item in self.vars.iter_mut() {
            item.source = source.clone();
        }
    }

    /// this needs to be called during checkout
    pub fn repopulate_inherited_vars(&mut self) -> anyhow::Result<()> {
        // Try to populate the remaining vars with variables from the
        // environment
        for any in self.vars.iter_mut() {
            if let Value::Inherit(inherit_value) = &mut any.value {
                let env_value = inherit_value
                    .get_value(&any.name)
                    .context(format_context!("When getting inherit value"))?;

                if let Some(EnvBool::Yes) = inherit_value.is_save_at_checkout {
                    inherit_value.value = env_value;
                }
            }

            if let Value::Script(script_value) = &mut any.value {
                let env_value = script_value
                    .get_value(&any.name)
                    .context(format_context!("When getting script value"))?;

                if let Some(EnvBool::Yes) = script_value.is_save_at_checkout {
                    script_value.value = env_value;
                }
            }
        }
        Ok(())
    }

    fn get_secrets(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut secret_map = HashMap::new();
        for any in self.vars.iter() {
            match &any.value {
                Value::Inherit(inherit_value) => {
                    if let (Some(EnvBool::Yes), Some(value)) = (
                        inherit_value.is_secret,
                        inherit_value.get_value(any.name.as_ref()).ok().flatten(),
                    ) {
                        secret_map.insert(any.name.clone(), value.clone());
                    }
                }
                Value::Script(script_value) => {
                    if let (Some(EnvBool::Yes), Some(value)) = (
                        script_value.is_secret,
                        script_value.get_value(any.name.as_ref()).ok().flatten(),
                    ) {
                        secret_map.insert(any.name.clone(), value.clone());
                    }
                }
                _ => (),
            }
        }
        Ok(secret_map)
    }

    pub fn get_secret_values(&self) -> anyhow::Result<Vec<Arc<str>>> {
        let result = self
            .get_secrets()
            .context(format_context!("While getting secret values"))?
            .values()
            .cloned()
            .collect();
        Ok(result)
    }

    pub fn get_vars(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut result = HashMap::new();
        let mut assign_from_args = HashMap::new();
        for any in self.vars.iter() {
            let name = any.name.clone();
            match &any.value {
                Value::None => {}
                Value::Assign(assign_value) => {
                    result.insert(name, assign_value.value.clone());
                }
                Value::Automatic => {
                    result.insert(name.clone(), Value::get_automatic_placeholder(&name).into());
                }
                Value::Inherit(inherit_value) => {
                    let env_value = inherit_value
                        .get_value(&name)
                        .context(format_context!("while getting value for env {name}"))?;

                    if let Some(value) = env_value {
                        result.insert(name, value.clone());
                    }
                }
                Value::Script(script_value) => {
                    let env_value = script_value
                        .get_value(&name)
                        .context(format_context!("while getting value for env {name}"))?;

                    if let Some(value) = env_value {
                        result.insert(name, value.clone());
                    }
                }
                Value::Prepend(AppendPrependValue { value, separator }) => {
                    if let Some(entry) = result.get_mut(&name) {
                        let prepended = format!("{value}{separator}{entry}");
                        *entry = prepended.into();
                    } else {
                        result.insert(name, value.clone());
                    }
                }
                Value::Append(AppendPrependValue { value, separator }) => {
                    if let Some(entry) = result.get_mut(&name) {
                        let prepended = format!("{entry}{separator}{value}");
                        *entry = prepended.into();
                    } else {
                        result.insert(name, value.clone());
                    }
                }
                Value::AssignFromArg(value) => {
                    assign_from_args.insert(name, value.clone());
                }
            }
        }

        // override vars assigned from the command line
        for (name, value) in assign_from_args {
            result.insert(name, value);
        }

        Ok(result)
    }

    fn to_yaml(&self, predicate: impl Fn(&Any) -> bool) -> anyhow::Result<String> {
        let any_yaml = serde_yaml::to_string(
            &self
                .vars
                .iter()
                .filter(|any| predicate(any))
                .collect::<Vec<_>>(),
        )
        .context(format_context!("failed to create yaml for list value"))?;
        Ok(any_yaml)
    }

    pub fn to_markdown(&self, auto_vars: &HashMap<&str, Arc<str>>) -> anyhow::Result<String> {
        let mut result = String::new();
        result.push_str(markdown::heading(1, "Environment Variables").as_str());
        result.push_str(markdown::heading(2, "Assign Variables").as_str());
        result.push_str(markdown::paragraph(ASSIGN_VARIABLES_DESCRIPTION).as_str());

        let any_yaml = self
            .to_yaml(|any| matches!(any.value, Value::Assign(_)))
            .context(format_context!("failed to create yaml for list value"))?;
        result.push_str(markdown::code_block("yaml", &any_yaml).as_str());

        result.push('\n');
        result.push_str(markdown::heading(2, "List Variables").as_str());
        result.push_str(markdown::paragraph(LIST_VARIABLES_DESCRIPTION).as_str());

        let any_yaml = self
            .to_yaml(|any| {
                matches!(any.value, Value::Append(_)) || matches!(any.value, Value::Prepend(_))
            })
            .context(format_context!("failed to create yaml for list value"))?;
        result.push_str(markdown::code_block("yaml", &any_yaml).as_str());

        result.push('\n');
        result.push_str(markdown::heading(2, "Inherit Variables").as_str());
        result.push_str(markdown::paragraph(INHERIT_VARIABLES_DESCRIPTION).as_str());

        let any_yaml = self
            .to_yaml(|any| matches!(any.value, Value::Inherit(_)))
            .context(format_context!("failed to create yaml for list value"))?;
        result.push_str(markdown::code_block("yaml", &any_yaml).as_str());

        result.push('\n');
        result.push_str(markdown::heading(2, "Script Variables").as_str());
        result.push_str(markdown::paragraph(SCRIPT_VARIABLES_DESCRIPTION).as_str());

        let any_yaml = self
            .to_yaml(|any| matches!(any.value, Value::Script(_)))
            .context(format_context!("failed to create yaml for script value"))?;
        result.push_str(markdown::code_block("yaml", &any_yaml).as_str());

        result.push('\n');
        result.push_str(markdown::heading(2, "Assign From Command Line Arguments").as_str());
        result.push_str(markdown::paragraph(ASSIGN_FROM_COMMAND_LINE_DESCRIPTION).as_str());

        let any_yaml = self
            .to_yaml(|any| matches!(any.value, Value::AssignFromArg(_)))
            .context(format_context!("failed to create yaml for list value"))?;
        result.push_str(markdown::code_block("yaml", &any_yaml).as_str());

        result.push('\n');
        result.push_str(markdown::heading(2, "Automatic Variables").as_str());
        result.push_str(markdown::paragraph(AUTOMATIC_VARIABLES_DESCRIPTION).as_str());

        let any_yaml = self
            .to_yaml(|any| matches!(any.value, Value::Automatic))
            .context(format_context!("failed to create yaml for automatic value"))?;
        result.push_str(markdown::code_block("yaml", &any_yaml).as_str());

        if !auto_vars.is_empty() {
            result.push('\n');
            result.push_str(markdown::heading(2, "Automatic Variable Values").as_str());
            let mut shell_code = String::new();
            for (key, value) in auto_vars.iter() {
                shell_code.push_str(&format!("{key}={value}\n"));
            }
            result.push_str(markdown::code_block("shell", &shell_code).as_str());
        }

        result.push_str(markdown::heading(2, "Workspace Environment").as_str());
        let vars = self
            .get_vars()
            .context(format_context!("while getting vars"))?;
        let mut shell_code = String::new();
        for (key, value) in vars {
            shell_code.push_str(&format!("{key}={value}\n"));
        }
        result.push_str(markdown::code_block("shell", &shell_code).as_str());

        let secrets = self
            .get_secret_values()
            .context(format_context!("Failed to get secrets"))?;

        for secret in secrets {
            result = result.replace(secret.as_ref(), "REDACTED");
        }

        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vars: Option<HashMap<Arc<str>, Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_paths: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_vars: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional_inherited_vars: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_inherited_vars: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_inherited_vars: Option<Vec<Arc<str>>>,
}

impl From<&Environment> for AnyEnvironment {
    fn from(env: &Environment) -> AnyEnvironment {
        let mut any_env = AnyEnvironment::default();
        if let Some(vars) = &env.vars {
            for (key, value) in vars {
                any_env
                    .vars
                    .push(Any::new_set_value(key.clone(), value.clone()));
            }
        }
        if let Some(paths) = &env.paths {
            for path in paths {
                any_env.vars.push(Any::new_path_value(path.clone()));
            }
        }
        if let Some(paths) = &env.system_paths {
            for path in paths {
                any_env.vars.push(Any::new_system_path_value(path.clone()));
            }
        }
        if let Some(vars) = &env.inherited_vars {
            for item in vars {
                any_env.vars.push(Any::new_inherit_value(item.clone()));
            }
        }
        if let Some(vars) = &env.optional_inherited_vars {
            for item in vars {
                any_env.vars.push(Any::new_try_inherit_value(item.clone()));
            }
        }
        if let Some(vars) = &env.secret_inherited_vars {
            for item in vars {
                any_env.vars.push(Any::new_secret_value(item.clone()));
            }
        }
        if let Some(vars) = &env.run_inherited_vars {
            for item in vars {
                any_env.vars.push(Any::new_secret_value(item.clone()));
            }
        }
        any_env
    }
}
