#[allow(unused)]

use anyhow::Context;
#[allow(unused)]
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArchiveDriver {
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.bz2")]
    TarBz2,
    #[serde(rename = "tar.7z")]
    Tar7z,
    #[serde(rename = "zip")]
    Zip,
}

impl ArchiveDriver {
    #[allow(unused)]
    fn get_extension(&self) -> &'static str {
        match self {
            ArchiveDriver::TarGz => "tar.gz",
            ArchiveDriver::TarBz2 => "tar.bz2",
            ArchiveDriver::Tar7z => "tar.7z",
            ArchiveDriver::Zip => "zip",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub input: String,
    pub name: String,
    pub version: String,
    pub driver: ArchiveDriver,
    pub platform: Option<platform::Platform>,
    pub includes: Option<Vec<String>>,
    pub excludes: Option<Vec<String>>,
}

impl Archive {
    #[allow(unused)]
    pub fn get_output_file(&self) -> String {
        let mut result = format!("{}-{}", self.name, self.version);
        if let Some(platform) = self.platform.as_ref() {
            result.push_str(format!("-{}", platform).as_str());
        }
        result.push('.');
        result.push_str(self.driver.get_extension());
        result
    }

    pub fn execute(
        &self,
        _name: &str,
        _progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {


        Ok(())
    }
}
