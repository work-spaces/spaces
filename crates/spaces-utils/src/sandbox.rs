use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Network access policy for a sandboxed rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkPolicy {
    /// All network access is blocked.
    Blocked,
    /// No network restrictions.
    #[default]
    Unrestricted,
}

/// Filesystem sandbox manifest for a single build rule.
///
/// Maps directly onto a rule's declared inputs and outputs:
/// - `read`  → rule inputs (read-only)
/// - `write` → rule outputs (read-write)
/// - `exec`  → tools/compilers invoked (read-only, semantically distinct from data inputs)
///
/// Call [`Manifest::resolve`] to expand relative and env-var paths before
/// calling [`Manifest::apply`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sandbox {
    /// Rule name used in sandbox violation diagnostics.
    pub name: Arc<str>,
    /// Paths (files or directories) that may be read — rule inputs.
    #[serde(default)]
    pub read: Vec<Arc<str>>,
    /// Paths (files or directories) that may be written — rule outputs.
    /// Grants read-write access.
    #[serde(default)]
    pub write: Vec<Arc<str>>,
    /// Paths to executables and toolchain directories the rule may invoke.
    /// Grants read-only access, kept separate from data inputs for clarity.
    #[serde(default)]
    pub exec: Vec<Arc<str>>,
    /// Scratch/temp directory for intermediate artifacts. Grants read-write access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scratch: Option<Arc<str>>,
    /// Network access policy. Defaults to `Unrestricted`; set to `Blocked`
    /// for compile/link rules that have no reason to reach the network.
    #[serde(default)]
    pub network: NetworkPolicy,
    /// Paths that must never be granted access, even if they fall inside a
    /// broad `read` or `write` grant (e.g., credential files inside a home dir).
    ///
    /// On Linux/Landlock the allowlist model means sub-path denial requires
    /// splitting broad grants into explicit per-directory entries. Declaring
    /// deny paths here causes [`Manifest::apply`] to return an error if any
    /// `read` or `write` path is an ancestor of a deny path, forcing the rule
    /// author to use narrower grants instead.
    #[serde(default)]
    pub deny: Vec<Arc<str>>,
}

impl Sandbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_name(mut self, name: impl Into<Arc<str>>) -> Self {
        self.name = name.into();
        self
    }

    pub fn allow_read(mut self, path: impl Into<Arc<str>>) -> Self {
        self.read.push(path.into());
        self
    }

    pub fn allow_write(mut self, path: impl Into<Arc<str>>) -> Self {
        self.write.push(path.into());
        self
    }

    pub fn allow_exec(mut self, path: impl Into<Arc<str>>) -> Self {
        self.exec.push(path.into());
        self
    }

    pub fn with_scratch(mut self, path: impl Into<Arc<str>>) -> Self {
        self.scratch = Some(path.into());
        self
    }

    pub fn with_network(mut self, policy: NetworkPolicy) -> Self {
        self.network = policy;
        self
    }

    pub fn deny_path(mut self, path: impl Into<Arc<str>>) -> Self {
        self.deny.push(path.into());
        self
    }

    /// Merge all paths from `other` into `self`. `other`'s `system` and
    /// `network` fields are OR'd / take precedence when more permissive.
    pub fn extend(&mut self, other: &Sandbox) {
        self.read.extend(other.read.iter().cloned());
        self.write.extend(other.write.iter().cloned());
        self.exec.extend(other.exec.iter().cloned());
        self.deny.extend(other.deny.iter().cloned());
        if let Some(ref s) = other.scratch {
            self.scratch.get_or_insert_with(|| s.clone());
        }
        if other.network == NetworkPolicy::Unrestricted {
            self.network = NetworkPolicy::Unrestricted;
        }
    }

    /// Resolve all paths in-place relative to `base`.
    /// Absolute paths are kept as-is.
    pub fn resolve(&mut self, base: &Path) -> anyhow::Result<()> {
        let resolve_list = |paths: &mut Vec<Arc<str>>| -> anyhow::Result<()> {
            for path in paths.iter_mut() {
                let p = Path::new(path.as_ref());
                if p.is_relative() {
                    *path = base
                        .join(p)
                        .to_str()
                        .context(format_context!("path is not valid UTF-8"))?
                        .into();
                }
            }
            Ok(())
        };

        resolve_list(&mut self.read)?;
        resolve_list(&mut self.write)?;
        resolve_list(&mut self.exec)?;
        resolve_list(&mut self.deny)?;
        if let Some(ref mut s) = self.scratch {
            let p = Path::new(s.as_ref());
            if p.is_relative() {
                *s = base
                    .join(p)
                    .to_str()
                    .context(format_context!("scratch path is not valid UTF-8"))?
                    .into();
            }
        }
        Ok(())
    }

    /// Convert this manifest into a nono `nono::CapabilitySet`.
    ///
    /// Returns an error if any `read` or `write` path is an ancestor of a
    /// `deny` path — the rule author must use narrower grants in that case.
    fn to_capability_set(&self) -> anyhow::Result<nono::CapabilitySet> {
        self.check_deny_conflicts()?;

        let mut caps = nono::CapabilitySet::new();

        for path in &self.read {
            caps = caps
                .allow_path(&**path, nono::AccessMode::Read)
                .with_context(|| format_context!("failed to allow read for: {path}"))?;
        }

        for path in &self.write {
            caps = caps
                .allow_path(&**path, nono::AccessMode::ReadWrite)
                .with_context(|| format_context!("failed to allow write for: {path}"))?;
        }

        for path in &self.exec {
            caps = caps
                .allow_path(&**path, nono::AccessMode::Read)
                .with_context(|| format_context!("failed to allow exec for: {path}"))?;
        }

        if let Some(ref s) = self.scratch {
            caps = caps
                .allow_path(&**s, nono::AccessMode::ReadWrite)
                .with_context(|| format_context!("failed to allow scratch for: {s}"))?;
        }

        caps = match self.network {
            NetworkPolicy::Blocked => caps.set_network_mode(nono::NetworkMode::Blocked),
            NetworkPolicy::Unrestricted => caps.set_network_mode(nono::NetworkMode::AllowAll),
        };

        Ok(caps)
    }

    /// Apply the sandbox defined by this manifest to the current process.
    ///
    /// This is irreversible. After this call, the process can only access
    /// the paths and network modes declared in this manifest.
    pub fn apply(&self) -> anyhow::Result<()> {
        let support = nono::Sandbox::support_info();
        if !support.is_supported {
            return Err(format_error!(
                "sandboxing is not supported on this platform: {}",
                support.details
            ));
        }
        let caps = self.to_capability_set()?;
        nono::Sandbox::apply(&caps)
            .with_context(|| format_context!("failed to apply sandbox `{}`", self.name))?;
        Ok(())
    }

    /// Return an error if any `read` or `write` path is an ancestor of a
    /// `deny` path. Broad grants that cover a denied sub-path can't be
    /// split by the Landlock allowlist model; the rule must use narrower grants.
    fn check_deny_conflicts(&self) -> anyhow::Result<()> {
        for deny in &self.deny {
            let deny_path = PathBuf::from(&**deny);
            for grant in self.read.iter().chain(self.write.iter()) {
                let grant_path = PathBuf::from(&**grant);
                if deny_path.starts_with(&grant_path) {
                    return Err(format_error!(
                        "deny path '{deny}' is covered by grant '{grant}'; \
                         use narrower grants to exclude it"
                    ));
                }
            }
        }
        Ok(())
    }
}
