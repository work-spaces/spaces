use crate::{singleton, workspace};
use anyhow_source_location::format_error;
use std::sync::Arc;

pub fn get_source_from_label(label: &str) -> String {
    let (source, _rule_name) = label.split_once(":").unwrap_or((label, ""));
    let source = source.strip_prefix("//").unwrap_or(source);
    let source_dot = format!("{source}.spaces.star");
    let source_slash = format!("{source}/spaces.star");
    if std::path::Path::new(source_dot.as_str()).exists() {
        source_dot
    } else if std::path::Path::new(source_slash.as_str()).exists() {
        source_slash
    } else {
        source.to_string()
    }
}

pub fn sanitize_rule_for_display(rule_name: Arc<str>) -> Arc<str> {
    // if length > MAX_RULE_NAME_LENGTH show firtst INTRO_LENGTH chars, then ... then the rest
    const MAX_RULE_NAME_LENGTH: usize = 64;
    const INTRO_LENGTH: usize = 16;
    const RULE_NAME_REST: usize = MAX_RULE_NAME_LENGTH - INTRO_LENGTH;
    if rule_name.len() < MAX_RULE_NAME_LENGTH {
        return rule_name;
    }

    let mut result = String::new();
    for (i, c) in rule_name.chars().enumerate() {
        if i < INTRO_LENGTH {
            result.push(c);
        } else if i == INTRO_LENGTH {
            result.push_str("...");
        } else if i > rule_name.len() - RULE_NAME_REST {
            result.push(c);
        }
    }
    result.into()
}

pub fn sanitize_rule(rule_name: Arc<str>, starlark_module: Option<Arc<str>>) -> Arc<str> {
    if is_rule_sanitized(rule_name.as_ref()) {
        return rule_name;
    }

    if let Some(latest_module) = starlark_module {
        let slash_suffix = format!("/{}", workspace::SPACES_MODULE_NAME);
        let dot_suffix = format!(".{}", workspace::SPACES_MODULE_NAME);

        let rule_prefix = latest_module
            .strip_suffix(slash_suffix.as_str())
            .or_else(|| latest_module.strip_suffix(dot_suffix.as_str()))
            .unwrap_or("");

        let separator = if rule_name.contains(':') { '/' } else { ':' };
        format!("//{rule_prefix}{separator}{rule_name}").into()
    } else {
        rule_name
    }
}

pub fn sanitize_glob_value(
    value: &str,
    rule_name: &str,
    starlark_module: Option<Arc<str>>,
) -> anyhow::Result<Arc<str>> {
    let module = starlark_module.unwrap_or("unknown".into());
    if value.starts_with("+//**") {
        singleton::push_glob_warning(
            format!(
                "{module}:{rule_name} inputs -> {value} globbing the workspace root is bad for performance"
            )
            .into()
        );
    }

    if value.starts_with("+//") || value.starts_with("-//") {
        return Ok(value.replace("+//", "+").replace("-//", "-").into());
    }

    Err(format_error!(
        "{}:{} inputs -> {} must begin with +// or -// and be a workspace root path",
        module,
        rule_name,
        value
    ))
}

pub fn sanitize_working_directory(
    working_directory: Arc<str>,
    starlark_module: Option<Arc<str>>,
) -> Arc<str> {
    if working_directory.starts_with("//") {
        return working_directory;
    }
    if let Some(latest_module) = starlark_module {
        let module_path = std::path::Path::new(latest_module.as_ref());
        let path_prefix = module_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let separator = if path_prefix.is_empty() { "" } else { "/" };
        format!("//{path_prefix}{separator}{working_directory}").into()
    } else {
        // The latest module is always available, this path is unreachable
        // unless there is a bug
        working_directory
    }
}

pub fn is_rule_sanitized(rule_name: &str) -> bool {
    rule_name.starts_with("//")
}
