use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "macos-x86_64")]
    MacosX86_64,
    #[serde(rename = "macos-aarch64")]
    MacosAarch64,
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
    #[serde(rename = "windows-aarch64")]
    WindowsAarch64,
    #[serde(rename = "linux-x86_64")]
    LinuxX86_64,
    #[serde(rename = "linux-aarch64")]
    LinuxAarch64,
}

impl Platform {
    pub fn get_platform() -> Option<Platform> {
        if cfg!(target_os = "macos") {
            if cfg!(target_arch = "x86_64") {
                return Some(Self::MacosX86_64);
            } else if cfg!(target_arch = "aarch64") {
                return Some(Self::MacosAarch64);
            }
        } else if cfg!(target_os = "windows") {
            if cfg!(target_arch = "x86_64") {
                return Some(Self::WindowsX86_64);
            } else if cfg!(target_arch = "aarch64") {
                return Some(Self::WindowsAarch64);
            }
        } else if cfg!(target_os = "linux") {
            if cfg!(target_arch = "x86_64") {
                return Some(Self::LinuxX86_64);
            } else if cfg!(target_arch = "aarch64") {
                return Some(Self::LinuxAarch64);
            }
        }
        None
    }

    #[allow(dead_code)]
    pub fn is_windows() -> bool {
        matches!(Self::get_platform(), Some(Self::WindowsX86_64) | Some(Self::WindowsAarch64))
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::MacosX86_64 => write!(f, "macos-x86_64"),
            Platform::MacosAarch64 => write!(f, "macos-aarch64"),
            Platform::WindowsX86_64 => write!(f, "windows-x86_64"),
            Platform::WindowsAarch64 => write!(f, "windows-aarch64"),
            Platform::LinuxX86_64 => write!(f, "linux-x86_64"),
            Platform::LinuxAarch64 => write!(f, "linux-aarch64"),
        }
    }
}