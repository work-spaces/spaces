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
/// Call [`Sandbox::resolve`] to expand relative paths before calling
/// [`Sandbox::apply`].
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
    /// deny paths here causes [`Sandbox::apply`] to return an error if any
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

    /// Merge all paths from `other` into `self`. For `network`, the more
    /// restrictive policy wins (`Blocked` takes precedence over `Unrestricted`).
    pub fn extend(&mut self, other: &Sandbox) {
        self.read.extend(other.read.iter().cloned());
        self.write.extend(other.write.iter().cloned());
        self.exec.extend(other.exec.iter().cloned());
        self.deny.extend(other.deny.iter().cloned());
        if let Some(ref s) = other.scratch {
            self.scratch.get_or_insert_with(|| s.clone());
        }
        if other.network == NetworkPolicy::Blocked {
            self.network = NetworkPolicy::Blocked;
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

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // resolve() tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_resolve_relative_paths() {
        let mut sandbox = Sandbox::new()
            .allow_read("src")
            .allow_write("out")
            .allow_exec("tools/bin")
            .with_scratch("tmp")
            .deny_path("secrets");

        let base = Path::new("/workspace/project");
        sandbox.resolve(base).unwrap();

        assert_eq!(&*sandbox.read[0], "/workspace/project/src");
        assert_eq!(&*sandbox.write[0], "/workspace/project/out");
        assert_eq!(&*sandbox.exec[0], "/workspace/project/tools/bin");
        assert_eq!(
            sandbox.scratch.as_ref().map(|s| &**s),
            Some("/workspace/project/tmp")
        );
        assert_eq!(&*sandbox.deny[0], "/workspace/project/secrets");
    }

    #[test]
    fn test_resolve_absolute_paths_unchanged() {
        let mut sandbox = Sandbox::new()
            .allow_read("/absolute/read")
            .allow_write("/absolute/write")
            .allow_exec("/usr/bin")
            .with_scratch("/tmp/scratch")
            .deny_path("/etc/passwd");

        let base = Path::new("/workspace/project");
        sandbox.resolve(base).unwrap();

        // Absolute paths should remain unchanged
        assert_eq!(&*sandbox.read[0], "/absolute/read");
        assert_eq!(&*sandbox.write[0], "/absolute/write");
        assert_eq!(&*sandbox.exec[0], "/usr/bin");
        assert_eq!(sandbox.scratch.as_ref().map(|s| &**s), Some("/tmp/scratch"));
        assert_eq!(&*sandbox.deny[0], "/etc/passwd");
    }

    #[test]
    fn test_resolve_mixed_paths() {
        let mut sandbox = Sandbox::new()
            .allow_read("relative/path")
            .allow_read("/absolute/path")
            .allow_write("output")
            .allow_write("/var/output");

        let base = Path::new("/base");
        sandbox.resolve(base).unwrap();

        assert_eq!(&*sandbox.read[0], "/base/relative/path");
        assert_eq!(&*sandbox.read[1], "/absolute/path");
        assert_eq!(&*sandbox.write[0], "/base/output");
        assert_eq!(&*sandbox.write[1], "/var/output");
    }

    #[test]
    fn test_resolve_empty_sandbox() {
        let mut sandbox = Sandbox::new();
        let base = Path::new("/workspace");
        // Should succeed with no paths to resolve
        assert!(sandbox.resolve(base).is_ok());
    }

    #[test]
    fn test_resolve_without_scratch() {
        let mut sandbox = Sandbox::new().allow_read("input");
        let base = Path::new("/workspace");
        sandbox.resolve(base).unwrap();

        assert!(sandbox.scratch.is_none());
        assert_eq!(&*sandbox.read[0], "/workspace/input");
    }

    // -------------------------------------------------------------------------
    // check_deny_conflicts() tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_check_deny_conflicts_no_conflict() {
        let sandbox = Sandbox::new()
            .allow_read("/home/user/project/src")
            .allow_write("/home/user/project/out")
            .deny_path("/home/user/.ssh");

        // No conflict: deny path is not under any grant
        assert!(sandbox.check_deny_conflicts().is_ok());
    }

    #[test]
    fn test_check_deny_conflicts_read_covers_deny() {
        let sandbox = Sandbox::new()
            .allow_read("/home/user")
            .deny_path("/home/user/.ssh/id_rsa");

        let result = sandbox.check_deny_conflicts();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("/home/user/.ssh/id_rsa"));
        assert!(err_msg.contains("/home/user"));
        assert!(err_msg.contains("narrower grants"));
    }

    #[test]
    fn test_check_deny_conflicts_write_covers_deny() {
        let sandbox = Sandbox::new()
            .allow_write("/var/data")
            .deny_path("/var/data/secrets/key.pem");

        let result = sandbox.check_deny_conflicts();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("/var/data/secrets/key.pem"));
        assert!(err_msg.contains("/var/data"));
    }

    #[test]
    fn test_check_deny_conflicts_multiple_denies_one_conflict() {
        let sandbox = Sandbox::new()
            .allow_read("/home/user/project")
            .deny_path("/etc/passwd") // no conflict
            .deny_path("/home/user/project/.env"); // conflict!

        let result = sandbox.check_deny_conflicts();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("/home/user/project/.env"));
    }

    #[test]
    fn test_check_deny_conflicts_exact_match() {
        // Deny path exactly matches a grant path
        let sandbox = Sandbox::new()
            .allow_read("/home/user/file.txt")
            .deny_path("/home/user/file.txt");

        // starts_with returns true for exact match, so this should error
        let result = sandbox.check_deny_conflicts();
        assert!(result.is_err());
    }

    #[test]
    fn test_check_deny_conflicts_sibling_paths_ok() {
        // Sibling paths should not conflict
        let sandbox = Sandbox::new()
            .allow_read("/home/user/project")
            .deny_path("/home/user/secrets");

        // /home/user/secrets does NOT start with /home/user/project
        assert!(sandbox.check_deny_conflicts().is_ok());
    }

    #[test]
    fn test_check_deny_conflicts_empty_deny_list() {
        let sandbox = Sandbox::new()
            .allow_read("/home/user")
            .allow_write("/var/data");

        // No deny paths means no conflicts
        assert!(sandbox.check_deny_conflicts().is_ok());
    }

    #[test]
    fn test_check_deny_conflicts_empty_grants() {
        let sandbox = Sandbox::new().deny_path("/etc/passwd");

        // No grants means deny path can't be covered
        assert!(sandbox.check_deny_conflicts().is_ok());
    }

    // -------------------------------------------------------------------------
    // Builder / extend tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_extend_merges_paths() {
        let mut base = Sandbox::new()
            .allow_read("/a")
            .allow_write("/b")
            .with_network(NetworkPolicy::Unrestricted);

        let other = Sandbox::new()
            .allow_read("/c")
            .allow_write("/d")
            .allow_exec("/usr/bin")
            .deny_path("/secret");

        base.extend(&other);

        assert_eq!(base.read.len(), 2);
        assert_eq!(base.write.len(), 2);
        assert_eq!(base.exec.len(), 1);
        assert_eq!(base.deny.len(), 1);
    }

    #[test]
    fn test_extend_blocked_network_wins() {
        let mut base = Sandbox::new().with_network(NetworkPolicy::Unrestricted);
        let other = Sandbox::new().with_network(NetworkPolicy::Blocked);

        base.extend(&other);
        assert_eq!(base.network, NetworkPolicy::Blocked);
    }

    #[test]
    fn test_extend_unrestricted_does_not_override_blocked() {
        let mut base = Sandbox::new().with_network(NetworkPolicy::Blocked);
        let other = Sandbox::new().with_network(NetworkPolicy::Unrestricted);

        base.extend(&other);
        assert_eq!(base.network, NetworkPolicy::Blocked);
    }

    #[test]
    fn test_extend_scratch_uses_first() {
        let mut base = Sandbox::new().with_scratch("/tmp/first");
        let other = Sandbox::new().with_scratch("/tmp/second");

        base.extend(&other);
        assert_eq!(base.scratch.as_ref().map(|s| &**s), Some("/tmp/first"));
    }

    #[test]
    fn test_extend_scratch_fills_none() {
        let mut base = Sandbox::new();
        let other = Sandbox::new().with_scratch("/tmp/scratch");

        base.extend(&other);
        assert_eq!(base.scratch.as_ref().map(|s| &**s), Some("/tmp/scratch"));
    }
}
