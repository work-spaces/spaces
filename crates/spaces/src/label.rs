use crate::workspace;
use std::sync::Arc;

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
        working_directory
    }
}

pub fn is_rule_sanitized(rule_name: &str) -> bool {
    rule_name.starts_with("//")
}
