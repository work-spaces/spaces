use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use printer::markdown;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

const SET_VARIABLES_DESCRIPTION: &str = r#"`Set` variables are assigned a value in the checkout rules. These values
may affect rule reproducibility. These values will be part of the rule digest unless filtered out.

Values passed to the command line using `--env=<NAME>=<VALUE>` will override the values in the checkout rules."#;

const SEPARATORS: &[&str] = &[",", ";", ":", "|", "_", "'", "-"];
const LIST_VARIABLES_DESCRIPTION: &str = r#"`List` variables work the same way as set variables when it comes to reproducibility.

`List` ENV values must be of the format:
- `<separator><value>`: value will be appended to the list
- `<value><separator>`: value will be prepeded to the list

`<separator>` must be one of"#;

const INHERIT_VARIABLES_DESCRIPTION: &str = r#"`Inherit` variables are inherited from the calling environment at checkout time.
The variable must be present in the calling environment or the checkout will fail.

These variables must may affect reproducibility. After checkout, they are treated the same as `Set` variables.

Values passed to the command line using `--env=<NAME>=<VALUE>` will override the values in the checkout rules."#;

const TRY_INHERIT_VARIABLES_DESCRIPTION: &str = r#"`TryInherit` variables are inherited from the calling environment at checkout time if available.
If the variable is not present in the calling environment, it will be silently ignored.

These variable may override `Set` variables. If the variable overrides the `Set` variable, they may affect reproducibility.
If the variable does not override a `Set` variable, they must not affect reproducibility.

Values passed to the command line using `--env=<NAME>=<VALUE>` will override the values in the checkout rules."#;

const SECRET_VARIABLES_DESCRIPTION: &str = r#"`Secret` variables are inherited from the calling environment at both checkout and run time.
The variable must be present in the calling environment or the operation will fail.

Secret values are redacted in logs and must not affect reproducibility."#;

const TRY_SECRET_VARIABLES_DESCRIPTION: &str = r#"`TrySecret` variables are inherited from the calling environment at both checkout and run time if available.
If the variable is not present in the calling environment, it will be silently ignored.

Secret values are redacted in logs and must not affect reproducibility."#;

const ASSIGN_FROM_COMMAND_LINE_DESCRIPTION: &str = r#"`AssignFromArg` variables are assigned by using the `--env=NAME=VALUE` option
during checkout and run time. Values passed during checkout persist in the workspace while values passed during run will only apply to
that run.

ENV values assigned from command line arguments will overwrite any other values."#;

pub fn calculate_digest(vars: &std::collections::HashMap<Arc<str>, Arc<str>>) -> String {
    let mut hasher = blake3::Hasher::new();
    let mut vars_list: Vec<String> = vars
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    vars_list.sort();
    for item in vars_list {
        hasher.update(item.as_bytes());
    }
    hasher.finalize().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum Where {
    #[default]
    After,
    Before,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub enum Value {
    /// Sets the value of the environment variable.
    /// May be overriden by a command line argument.
    /// Affects reproducibility.
    Assign(Arc<str>),
    /// First character will be dropped if this is the first entry.
    /// Put separator [,|:;] before to append and after to prepend.
    /// Affects reproducibility.
    List(Arc<str>),
    /// Inherit from the calling environment at checkout
    /// Must not affect reproducibility.
    Inherit(Option<Arc<str>>),
    /// Inherit from calling environment at checkout if available.
    /// Must not affect reproducibility.
    TryInherit(Option<Arc<str>>),
    #[default]
    /// Inherit at both checkout/run time and redact
    /// Must not affect reproducibility.
    Secret,
    /// Inherit at both checkout/run time, if available, and redact
    /// Must not affect reproducibility.
    TrySecret,
    /// Inherit at both checkout/run time, if available, and redact
    /// Must not affect reproducibility.
    AssignFromArg(Arc<str>),
}

impl Value {
    fn to_markdown(&self) -> String {
        match self {
            Value::Assign(value) => format!("Assign: `{value}`"),
            Value::List(value) => {
                let mut prepend_append = "Prepend";
                for sep in SEPARATORS {
                    if value.starts_with(sep) {
                        prepend_append = "Append";
                        break;
                    }
                }

                format!("List ({prepend_append}): `{value}`")
            }
            Value::Inherit(None) => "Inherit: `<not available>`".to_string(),
            Value::Inherit(Some(value)) => format!("Inherit: `{value}`"),
            Value::TryInherit(None) => "TryInherit: `<not available>`".to_string(),
            Value::TryInherit(Some(value)) => format!("TryInherit: `{value}`"),
            Value::Secret => "Secret".to_string(),
            Value::TrySecret => "TrySecret".to_string(),
            Value::AssignFromArg(value) => format!("AssignFromArg: `{value}`"),
        }
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
    pub description: Option<Arc<str>>,
}

impl Any {
    fn to_markdown(&self) -> String {
        let mut result = String::new();
        let name_item = format!("Name: `{}`", self.name);
        result.push_str(markdown::list_item(1, name_item.as_str()).as_str());
        if let Some(description) = &self.description {
            result.push_str(
                markdown::list_item(2, format!("Description: {description}").as_str()).as_str(),
            );
        }
        if let Some(source) = &self.source {
            result
                .push_str(markdown::list_item(2, format!("Source: `{source}`").as_str()).as_str());
        }

        result.push_str(markdown::list_item(2, self.value.to_markdown().as_str()).as_str());
        result
    }

    pub fn new_set_value(name: Arc<str>, value: Arc<str>) -> Self {
        Self {
            name,
            value: Value::Assign(value),
            description: None,
            source: None,
        }
    }

    pub fn new_inherit_value(name: Arc<str>) -> Self {
        Self {
            name,
            value: Value::Inherit(None),
            description: None,
            source: None,
        }
    }

    pub fn new_try_inherit_value(name: Arc<str>) -> Self {
        Self {
            name,
            value: Value::TryInherit(None),
            description: None,
            source: None,
        }
    }

    pub fn new_secret_value(name: Arc<str>) -> Self {
        Self {
            name,
            value: Value::Secret,
            description: None,
            source: None,
        }
    }

    pub fn new_path_value(path: Arc<str>) -> Self {
        Self {
            name: "PATH".into(),
            value: Value::List(format!("{path}:").into()),
            description: None,
            source: None,
        }
    }

    pub fn new_system_path_value(system_path: Arc<str>) -> Self {
        Self {
            name: "PATH".into(),
            value: Value::List(format!(":{system_path}").into()),
            description: None,
            source: None,
        }
    }

    fn process_list_entry(list_value: &mut String, list_entry: &str) -> anyhow::Result<()> {
        for sep in SEPARATORS {
            if let Some(entry) = list_entry.strip_prefix(sep) {
                if list_value.is_empty() {
                    *list_value = entry.into();
                } else {
                    list_value.push_str(list_entry);
                }
                return Ok(());
            }
            if let Some(entry) = list_entry.strip_suffix(sep) {
                if list_value.is_empty() {
                    *list_value = entry.into();
                } else {
                    let mut new_string = list_entry.to_string();
                    new_string.push_str(list_value.as_str());
                    *list_value = new_string;
                }
                return Ok(());
            }
        }
        Err(format_error!(
            "{list_entry} is an invalid env list value. Could not find a separator"
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReproducibleEnvironment {
    pub vars: HashMap<Arc<str>, Arc<str>>,
}

impl ReproducibleEnvironment {
    pub fn get_digest(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        let mut vars_list: Vec<_> = self
            .vars
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        vars_list.sort();
        for item in vars_list {
            hasher.update(item.as_bytes());
        }
        hasher.finalize().to_string()
    }
}

#[derive(Debug, Clone, Default)]
pub struct CheckoutEnvironment {
    pub vars: HashMap<Arc<str>, Arc<str>>,
}

#[derive(Debug, Clone, Default)]
pub struct RunEnvironment {
    pub vars: HashMap<Arc<str>, Arc<str>>,
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
        self.vars.iter().any(|v| v.name.as_ref() == name)
    }

    pub fn insert_or_update(&mut self, any: Any) {
        if let Some(index) = self.vars.iter().position(|v| v.name == any.name) {
            if matches!(self.vars[index].value, Value::List(_)) {
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
                description: None,
                source: Some("<command line argument>".into()),
            });
        }
    }

    pub fn append(&mut self, other: Self) {
        for any in other.vars {
            self.insert_or_update(any);
        }
    }

    pub fn populate_source_for_all(&mut self, source: Option<Arc<str>>) {
        for item in self.vars.iter_mut() {
            item.source = source.clone();
        }
    }

    fn get_set_vars(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut set_vars: HashMap<Arc<str>, Arc<str>> = HashMap::new();
        for item in self.vars.iter() {
            let name = item.name.clone();
            if let Value::Assign(value) = &item.value {
                set_vars.insert(name, value.clone());
            }
        }
        Ok(set_vars)
    }

    fn get_list_vars(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut list_map: HashMap<Arc<str>, Arc<str>> = HashMap::new();
        for item in self.vars.iter() {
            let name = item.name.clone();
            if let Value::List(item) = &item.value {
                let existing = list_map.entry(name).or_default();
                let mut existing_string: String = existing.to_string();
                Any::process_list_entry(&mut existing_string, item.as_ref())?;
                *existing = existing_string.into();
            }
        }
        Ok(list_map)
    }

    fn get_vars_from_args_map(&self) -> HashMap<Arc<str>, Any> {
        let mut vars_from_args = HashMap::new();
        for any in self.vars.iter() {
            if matches!(&any.value, Value::AssignFromArg(_)) {
                vars_from_args.insert(any.name.clone(), any.clone());
            }
        }
        vars_from_args
    }

    /// this needs to be called during checkout
    fn repopulate_inherited_vars(&mut self) -> anyhow::Result<()> {
        // first look to populate the inherited vars
        // with command line arguments. This will prevent
        // failure if the env is provided only on the command
        // line and not in the environment.
        let vars_from_args = self.get_vars_from_args_map();

        for any in self.vars.iter_mut() {
            if let Some(any_from_arg) = vars_from_args.get(&any.name) {
                if let Value::AssignFromArg(value_from_arg) = &any_from_arg.value {
                    match &any.value {
                        Value::Inherit(_) => {
                            any.value = Value::Inherit(Some(value_from_arg.clone()));
                        }
                        Value::TryInherit(_) => {
                            any.value = Value::Inherit(Some(value_from_arg.clone()));
                        }
                        _ => {}
                    }
                }
            }
        }

        // Try to populate the remaining vars with variables from the
        // environment
        for any in self.vars.iter_mut() {
            match &any.value {
                Value::Inherit(None) => {
                    let value = std::env::var(any.name.as_ref()).context(format_context!(
                        "Failed to inherit {} from the environment",
                        any.name
                    ))?;
                    any.value = Value::Inherit(Some(value.into()));
                }
                Value::TryInherit(None) => {
                    if let Ok(value) = std::env::var(any.name.as_ref()) {
                        any.value = Value::TryInherit(Some(value.into()));
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub fn repopulate(&mut self) -> anyhow::Result<()> {
        self.repopulate_inherited_vars()
            .context(format_context!("While repopulating ENV"))?;
        Ok(())
    }

    fn get_vars_from_args(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut args_vars = HashMap::new();
        for any in self.vars.iter() {
            if let Value::AssignFromArg(value) = &any.value {
                args_vars.insert(any.name.clone(), value.clone());
            }
        }
        Ok(args_vars)
    }

    fn get_inherited_vars(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut inherited_vars = HashMap::new();
        for any in self.vars.iter() {
            match &any.value {
                Value::Inherit(Some(value)) => {
                    inherited_vars.insert(any.name.clone(), value.clone());
                }
                Value::TryInherit(Some(value)) => {
                    inherited_vars.insert(any.name.clone(), value.clone());
                }
                _ => {}
            }
        }
        Ok(inherited_vars)
    }

    fn get_secrets(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut secret_map = HashMap::new();
        for any in self.vars.iter() {
            match &any.value {
                Value::Secret => {
                    let value = std::env::var(any.name.as_ref()).context(format_context!(
                        "Failed to inherit (secret) {} from the environment",
                        any.name
                    ))?;
                    secret_map.insert(any.name.clone(), value.into());
                }
                Value::TrySecret => {
                    if let Ok(value) = std::env::var(any.name.as_ref()) {
                        secret_map.insert(any.name.clone(), value.into());
                    }
                }
                _ => {}
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

    pub fn to_markdown(&self) -> String {
        let mut result = String::new();
        result.push_str(markdown::heading(1, "Environment Variables").as_str());
        result.push_str(markdown::heading(2, "Set Variables").as_str());
        result.push_str(markdown::paragraph(SET_VARIABLES_DESCRIPTION).as_str());

        for any in self.vars.iter() {
            if matches!(any.value, Value::Assign(_)) {
                result.push_str(any.to_markdown().as_str());
            }
        }

        result.push('\n');
        result.push_str(markdown::heading(2, "List Variables").as_str());
        let separators = SEPARATORS.join("");
        result.push_str(
            markdown::paragraph(format!("{LIST_VARIABLES_DESCRIPTION}{separators}").as_str())
                .as_str(),
        );

        for any in self.vars.iter() {
            if matches!(any.value, Value::List(_)) {
                result.push_str(any.to_markdown().as_str());
            }
        }

        result.push('\n');
        result.push_str(markdown::heading(2, "Inherit Variables").as_str());
        result.push_str(markdown::paragraph(INHERIT_VARIABLES_DESCRIPTION).as_str());

        for any in self.vars.iter() {
            if matches!(any.value, Value::Inherit(_)) {
                result.push_str(any.to_markdown().as_str());
            }
        }

        result.push('\n');
        result.push_str(markdown::heading(2, "Try Inherit Variables").as_str());
        result.push_str(markdown::paragraph(TRY_INHERIT_VARIABLES_DESCRIPTION).as_str());

        for any in self.vars.iter() {
            if matches!(any.value, Value::TryInherit(_)) {
                result.push_str(any.to_markdown().as_str());
            }
        }

        result.push('\n');
        result.push_str(markdown::heading(2, "Secret Variables").as_str());
        result.push_str(markdown::paragraph(SECRET_VARIABLES_DESCRIPTION).as_str());

        for any in self.vars.iter() {
            if matches!(any.value, Value::Secret) {
                result.push_str(any.to_markdown().as_str());
            }
        }

        result.push('\n');
        result.push_str(markdown::heading(2, "Try Secret Variables").as_str());
        result.push_str(markdown::paragraph(TRY_SECRET_VARIABLES_DESCRIPTION).as_str());

        for any in self.vars.iter() {
            if matches!(any.value, Value::TrySecret) {
                result.push_str(any.to_markdown().as_str());
            }
        }

        result.push('\n');
        result.push_str(markdown::heading(2, "Assign From Command Line Arguments").as_str());
        result.push_str(markdown::paragraph(ASSIGN_FROM_COMMAND_LINE_DESCRIPTION).as_str());

        for any in self.vars.iter() {
            if matches!(any.value, Value::AssignFromArg(_)) {
                result.push_str(any.to_markdown().as_str());
            }
        }

        result
    }

    pub fn get_run_environment(&self) -> anyhow::Result<RunEnvironment> {
        let checkout_env = CheckoutEnvironment::try_from(self)
            .context(format_context!("While getting checkout env for run env"))?;
        let mut run_env_vars = checkout_env.vars.clone();
        let secret_vars = self.get_secrets().context(format_context!(
            "Failed to get secret values for run environment"
        ))?;
        run_env_vars.extend(secret_vars);
        Ok(RunEnvironment { vars: run_env_vars })
    }
}

impl TryFrom<&AnyEnvironment> for ReproducibleEnvironment {
    type Error = anyhow::Error;
    fn try_from(any: &AnyEnvironment) -> anyhow::Result<ReproducibleEnvironment> {
        let mut vars = HashMap::new();

        let set_vars = any
            .get_set_vars()
            .context(format_context!("Failed to get set vars"))?;

        let list_vars = any
            .get_list_vars()
            .context(format_context!("Failed to get list vars"))?;

        let from_arg_vars = any
            .get_vars_from_args()
            .context(format_context!("Failed to get vars from args"))?;

        for any in any.vars.iter() {
            if let Value::Inherit(Some(value)) = &any.value {
                vars.insert(any.name.clone(), value.clone());
            }
        }

        // reproducibility does not consider try_inherit, secret or try_secret vars
        vars.extend(set_vars);
        vars.extend(list_vars);
        vars.extend(from_arg_vars);

        Ok(ReproducibleEnvironment { vars })
    }
}

impl TryFrom<&AnyEnvironment> for CheckoutEnvironment {
    type Error = anyhow::Error;
    fn try_from(any: &AnyEnvironment) -> anyhow::Result<Self> {
        let mut vars = HashMap::new();

        let set_vars = any
            .get_set_vars()
            .context(format_context!("Failed to get set vars"))?;

        let list_vars = any
            .get_list_vars()
            .context(format_context!("Failed to get list vars"))?;

        let inherited_vars = any.get_inherited_vars().context(format_context!(
            "While getting inherited vars for CheckoutEnvironment"
        ))?;

        let secret_vars = any.get_secrets().context(format_context!(
            "While getting secret vars for CheckoutEnvironment"
        ))?;

        let from_arg_vars = any
            .get_vars_from_args()
            .context(format_context!("Failed to get vars from args"))?;

        vars.extend(set_vars);
        vars.extend(list_vars);
        vars.extend(inherited_vars);
        vars.extend(secret_vars);
        vars.extend(from_arg_vars);

        Ok(Self { vars })
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub any: Option<Vec<Any>>,
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
        any_env
    }
}
