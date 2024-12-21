use crate::workspace;
use std::sync::Arc;

pub fn sanitize_rule(rule_name: Arc<str>, starlark_module: Option<Arc<str>>) -> Arc<str> {
    if let Some(latest_module) = starlark_module {
        let slash_suffix = format!("/{}", workspace::SPACES_MODULE_NAME);
        let dot_suffix = format!(".{}", workspace::SPACES_MODULE_NAME);

        let rule_prefix = latest_module
            .strip_suffix(slash_suffix.as_str())
            .or_else(|| latest_module.strip_suffix(dot_suffix.as_str()))
            .unwrap_or("");

        format!("{rule_prefix}:{rule_name}").into()
    } else {
        rule_name
    }
}

pub fn is_rule_sanitized(rule_name: &str) -> bool {
    rule_name.contains(':')
}
