use crate::workspace;

pub fn sanitize_rule(rule_name: &str, starlark_module: Option<&String>) -> String {
    if let Some(latest_module) = starlark_module {
        let rule_prefix = latest_module
            .strip_suffix(format!("/{}", workspace::SPACES_MODULE_NAME).as_str())
            .unwrap_or("");
        format!("{rule_prefix}:{rule_name}")
    } else {
        rule_name.to_string()
    }
}

pub fn is_rule_sanitized(rule_name: &str) -> bool {
    rule_name.contains(':')
}
