#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Ecode {
    DependencyGraphContainsCircularDependency = 1,
    FailedToEvaluateModule = 2,
    FailedToEvaluateModuleDuringCheckout = 3,
    TargetFileIsClaimedByMultipleRules = 5,
    TargetDirIsClaimedByMultipleRules = 6,
    TargetArtifactIsContainedInTargetDir = 7,
    TraceRunnerSyncFailed = 11,
    ExecutorTaskExecutionFailed = 12,
    ArchiveExecutorOperationFailed = 13,
    AssetExecutorOperationFailed = 14,
    EnvironmentExecutorOperationFailed = 15,
    ExecExecutorOperationFailed = 16,
    GitExecutorFailedToExecuteGitCommand = 17,
    FailedToCreateOrAcquireLockFile = 19,
    HttpArchiveExecutorOperationFailed = 20,
    OrasExecutorOperationFailed = 21,
    FailedToLoadJsonFilesManifest = 22,
    FailedToAddSoftLinkAsset = 28,
    FailedToAddAsset = 29,
    FailedToHardLinkAsset = 30,
    FailedToAddWhichAsset = 31,
    FailedToAddHomeAsset = 32,
}

impl Ecode {
    pub const ALL: [Self; 22] = [
        Self::DependencyGraphContainsCircularDependency,
        Self::FailedToEvaluateModule,
        Self::FailedToEvaluateModuleDuringCheckout,
        Self::TargetFileIsClaimedByMultipleRules,
        Self::TargetDirIsClaimedByMultipleRules,
        Self::TargetArtifactIsContainedInTargetDir,
        Self::TraceRunnerSyncFailed,
        Self::ExecutorTaskExecutionFailed,
        Self::ArchiveExecutorOperationFailed,
        Self::AssetExecutorOperationFailed,
        Self::EnvironmentExecutorOperationFailed,
        Self::ExecExecutorOperationFailed,
        Self::GitExecutorFailedToExecuteGitCommand,
        Self::FailedToCreateOrAcquireLockFile,
        Self::HttpArchiveExecutorOperationFailed,
        Self::OrasExecutorOperationFailed,
        Self::FailedToLoadJsonFilesManifest,
        Self::FailedToAddSoftLinkAsset,
        Self::FailedToAddAsset,
        Self::FailedToHardLinkAsset,
        Self::FailedToAddWhichAsset,
        Self::FailedToAddHomeAsset,
    ];

    pub const fn serial_number(self) -> u32 {
        self as u32
    }

    pub const fn from_serial_number(serial_number: u32) -> Option<Self> {
        match serial_number {
            1 => Some(Self::DependencyGraphContainsCircularDependency),
            2 => Some(Self::FailedToEvaluateModule),
            3 => Some(Self::FailedToEvaluateModuleDuringCheckout),
            5 => Some(Self::TargetFileIsClaimedByMultipleRules),
            6 => Some(Self::TargetDirIsClaimedByMultipleRules),
            7 => Some(Self::TargetArtifactIsContainedInTargetDir),
            11 => Some(Self::TraceRunnerSyncFailed),
            12 => Some(Self::ExecutorTaskExecutionFailed),
            13 => Some(Self::ArchiveExecutorOperationFailed),
            14 => Some(Self::AssetExecutorOperationFailed),
            15 => Some(Self::EnvironmentExecutorOperationFailed),
            16 => Some(Self::ExecExecutorOperationFailed),
            17 => Some(Self::GitExecutorFailedToExecuteGitCommand),
            19 => Some(Self::FailedToCreateOrAcquireLockFile),
            20 => Some(Self::HttpArchiveExecutorOperationFailed),
            21 => Some(Self::OrasExecutorOperationFailed),
            22 => Some(Self::FailedToLoadJsonFilesManifest),
            28 => Some(Self::FailedToAddSoftLinkAsset),
            29 => Some(Self::FailedToAddAsset),
            30 => Some(Self::FailedToHardLinkAsset),
            31 => Some(Self::FailedToAddWhichAsset),
            32 => Some(Self::FailedToAddHomeAsset),
            _ => None,
        }
    }

    pub const fn generic_message(self) -> &'static str {
        match self {
            Self::DependencyGraphContainsCircularDependency => {
                "dependency graph contains a circular dependency"
            }
            Self::FailedToEvaluateModule => "failed to evaluate module",
            Self::FailedToEvaluateModuleDuringCheckout => {
                "failed to evaluate module during checkout"
            }
            Self::TargetFileIsClaimedByMultipleRules => "target file is claimed by multiple rules",
            Self::TargetDirIsClaimedByMultipleRules => "target dir is claimed by multiple rules",
            Self::TargetArtifactIsContainedInTargetDir => {
                "target artifact is contained in target dir"
            }
            Self::TraceRunnerSyncFailed => "<trace> runner sync failed",
            Self::ExecutorTaskExecutionFailed => "executor task execution failed",
            Self::ArchiveExecutorOperationFailed => "archive executor operation failed",
            Self::AssetExecutorOperationFailed => "asset executor operation failed",
            Self::EnvironmentExecutorOperationFailed => "environment executor operation failed",
            Self::ExecExecutorOperationFailed => "exec executor operation failed",
            Self::GitExecutorFailedToExecuteGitCommand => {
                "git executor failed to execute git command"
            }
            Self::FailedToCreateOrAcquireLockFile => "failed to create/acquire lock file",
            Self::HttpArchiveExecutorOperationFailed => {
                "http_archive executor operation failed (try `spaces store fix`)"
            }
            Self::OrasExecutorOperationFailed => "oras executor operation failed",
            Self::FailedToLoadJsonFilesManifest => "failed to load json files manifest",
            Self::FailedToAddSoftLinkAsset => "failed to add soft link asset",
            Self::FailedToAddAsset => "failed to add asset",
            Self::FailedToHardLinkAsset => "failed to hard link asset",
            Self::FailedToAddWhichAsset => "failed to add which asset",
            Self::FailedToAddHomeAsset => "failed to add home asset",
        }
    }
}

impl From<Ecode> for u32 {
    fn from(value: Ecode) -> Self {
        value.serial_number()
    }
}

impl TryFrom<u32> for Ecode {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::from_serial_number(value).ok_or(())
    }
}

pub fn anyhow(ecode: Ecode, context: &str) -> anyhow::Error {
    let serial_number = ecode.serial_number();
    let mut result = format!("ecode[{serial_number:04}]: {}\n", ecode.generic_message());
    if !context.is_empty() {
        for line in context.lines() {
            result.push_str("  ");
            result.push_str(line);
            result.push('\n');
        }
    }
    anyhow::anyhow!(result)
}

pub fn anyhow_trace(ecode: Ecode) -> anyhow::Error {
    anyhow(ecode, "")
}

#[cfg(test)]
mod tests {
    use super::{Ecode, anyhow_trace};

    #[test]
    fn anyhow_trace_works_for_all_ecodes() {
        for ecode in Ecode::ALL {
            let _ = anyhow_trace(ecode);
        }
    }

    #[test]
    fn serial_number_round_trip_works_for_all_ecodes() {
        for ecode in Ecode::ALL {
            assert_eq!(
                Some(ecode),
                Ecode::from_serial_number(ecode.serial_number())
            );
        }
    }
}
