use crate::{singleton, workspace};
use std::sync::Arc;
use utils::labels;

pub use labels::{
    IsAnnotated, get_path_from_label, get_rule_name_from_label, get_source_from_label,
    is_rule_sanitized, sanitize_rule_for_display, sanitize_working_directory,
};

pub fn sanitize_rule(rule_name: Arc<str>, starlark_module: Option<Arc<str>>) -> Arc<str> {
    labels::sanitize_rule(rule_name, starlark_module, workspace::SPACES_MODULE_NAME)
}

pub fn sanitize_glob_value(
    value: &str,
    is_annotated: labels::IsAnnotated,
    rule_name: &str,
    starlark_module: Option<Arc<str>>,
) -> anyhow::Result<Arc<str>> {
    let module = starlark_module.clone().unwrap_or("unknown".into());
    if is_annotated == labels::IsAnnotated::Yes {
        if value.starts_with("+//**") {
            singleton::push_glob_warning(
                format!(
                    "{module}:{rule_name} inputs -> {value} globbing the workspace root is bad for performance"
                )
                .into()
            );
        }
    } else {
        if value.starts_with("//**") {
            singleton::push_glob_warning(
            format!(
                "{module}:{rule_name} inputs -> {value} globbing the workspace root is bad for performance"
            )
            .into()
        );
        }
    }

    labels::sanitize_glob_value(value, is_annotated, rule_name, starlark_module)
}
