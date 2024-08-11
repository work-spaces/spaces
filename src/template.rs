use crate::platform;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::Serialize;
use std::collections::HashMap;

// legacy
pub const SPACES_OVERLAY: &str = "{SPACES_OVERLAY}";
pub const SPACE: &str = "{SPACE}";
pub const USER: &str = "{USER}";
pub const UNIQUE: &str = "{UNIQUE}";
pub const SPACES_SYSROOT: &str = "{SPACES_SYSROOT}";
pub const SPACES_PLATFORM: &str = "{SPACES_PLATFORM}";
pub const SPACES_PATH: &str = "{SPACES_PATH}";
pub const SPACES_BRANCH: &str = "{SPACES_BRANCH}";

// use handlebars for latest

#[derive(Serialize, Default, Debug)]
pub struct Spaces {
    pub space_name: String,
    pub user: String,
    pub unique: String,
    pub sysroot: String,
    pub platform: String,
    pub path: String,
    pub branch: String,
    pub log_directory: String,
}

#[derive(Serialize, Debug)]
pub struct Model {
    pub spaces: Spaces,
    // parse all toml, json, and yaml files. Make them available for templating
    // String is the path of the file in the workspace
    pub files: HashMap<String, serde_json::Value>,
}

impl Model {
    fn get_unique() -> anyhow::Result<String> {
        let duration_since_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context(format_context!("No system time"))?;
        let duration_since_epoch_string = format!("{}", duration_since_epoch.as_nanos());
        let unique_sha256 = sha256::digest(duration_since_epoch_string.as_bytes());
        Ok(unique_sha256.as_str()[0..4].to_string())
    }

    pub fn new() -> anyhow::Result<Self> {
        let unique = Self::get_unique().context(format_context!(""))?;
        let platform = platform::Platform::get_platform()
            .context(format_context!("Unknown platform"))?
            .to_string();
        let user = std::env::var("USER").unwrap_or("NOUSER".to_string());

        Ok(Self {
            spaces: Spaces {
                unique,
                platform,
                user,
                ..Default::default()
            },
            files: HashMap::new(),
        })
    }

    pub fn set_space_directory(&mut self, path: &str){
        if let Some(space_name) = std::path::Path::new(path).file_name(){
            self.spaces.space_name = space_name.to_string_lossy().to_string();

            let sysroot = std::path::Path::new(path).join("sysroot");
            self.spaces.sysroot = sysroot.to_string_lossy().to_string();

            let log_directory = std::path::Path::new(path).join("spaces_logs");
            self.spaces.log_directory = log_directory.to_string_lossy().to_string();

        }
    }

    pub fn render_template_string(&self, template_contents: &str) -> anyhow::Result<String> {

        // add support for legacy replacements
        let legacy_replacements = maplit::hashmap! {
            SPACES_OVERLAY => "{{ spaces.overlay }}",
            SPACE => "{{ spaces.space_name }}",
            USER => "{{ spaces.user }}",
            UNIQUE => "{{ spaces.unique }}",
            SPACES_SYSROOT => "{{ spaces.sysroot }}",
            SPACES_PLATFORM => "{{ spaces.platform }}",
            SPACES_PATH => "{{ spaces.path }}",
            SPACES_BRANCH => "{{ spaces.branch }}",
        };

        let mut update_contents = template_contents.to_string();

        for (key, value) in legacy_replacements {
            update_contents = update_contents.replace(key, value);
        }

        let handlebars = handlebars::Handlebars::new();
        let rendered = handlebars
            .render_template(update_contents.as_str(), &self)
            .context(format_context!("Failed to render template contents"))?;

        Ok(rendered)
    }

    pub fn render_template_path(&self, template_path: &str) -> anyhow::Result<String> {
        let contents = std::fs::read_to_string(template_path)
            .context(format_context!("Failed to read template {template_path}"))?;

        let rendered = self
            .render_template_string(contents.as_str())
            .context(format_context!("Failed to render template {template_path}"))?;
        Ok(rendered)
    }


    #[allow(dead_code)]
    pub fn add_file(&mut self, path: &str) -> anyhow::Result<()> {
        let contents = std::fs::read_to_string(path)
            .context(format_context!("Failed to read file {path}"))?;

        let value = serde_json::from_str(contents.as_str())
            .context(format_context!("Failed to parse file {path}"))?;

        let mut sanitized_path = path.to_string();
        sanitized_path = sanitized_path.replace('/', "_");
        sanitized_path = sanitized_path.replace('.', "_");

        self.files.insert(sanitized_path, value);
        Ok(())
    }

}

#[cfg(test)]
mod test {
    use super::*;

    const UNIQUE: &str = "1234";

    #[test]
    fn test_template_model() {
        let mut model = Model::new().unwrap();

        model.spaces.space_name = "spaces-dev".to_string();
        model.files.insert(
            "spaces_Cargo_toml".to_string(),
            serde_json::json!({
                "package": {
                    "name": "spaces",
                    "version": "0.1.0",
                    "authors": ["user"]
                }
            }),
        );

        let template_string0 = r#"This is the space name {{ spaces.space_name }}"#;
        let template_string1 = r#"This is the space name {{spaces.space_name}}"#;
        let template_string_output = r#"This is the space name spaces-dev"#;

        let handlebars = handlebars::Handlebars::new();

        let rendered = handlebars
            .render_template(template_string0, &model)
            .unwrap();
        assert_eq!(rendered, template_string_output);

        let rendered = handlebars
            .render_template(template_string1, &model)
            .unwrap();
        assert_eq!(rendered, template_string_output);

        let template_string0 = r#"{{ spaces.space_name }} spaces version '{{ files.spaces_Cargo_toml.package.version }}'"#;
        let template_string0_output = r#"spaces-dev spaces version '0.1.0'"#;
        let rendered = handlebars
            .render_template(template_string0, &model)
            .unwrap();
        assert_eq!(rendered, template_string0_output);
    }


    #[test]
    fn test_model(){
        let mut model = Model::new().unwrap();

        model.spaces.space_name = "spaces-dev".to_string();
        model.spaces.unique = UNIQUE.to_string();
        model.spaces.sysroot = "test_data/spaces/spaces-dev/sysroot".to_string();
       
        assert_eq!(model.render_template_string(r#"{SPACES_SYSROOT}"#).unwrap(), "test_data/spaces/spaces-dev/sysroot");
        assert_eq!(model.render_template_string(r#"{SPACE}-{UNIQUE}"#).unwrap(), "spaces-dev-1234");
        assert_eq!(model.render_template_string(r#"{{ spaces.space_name }}-{{spaces.unique}}"#).unwrap(), "spaces-dev-1234");
        assert_eq!(model.render_template_string(r#"{{ spaces.sysroot }}"#).unwrap(), "test_data/spaces/spaces-dev/sysroot");

    }

}
