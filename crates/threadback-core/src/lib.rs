pub mod collectors;
pub mod domain;
pub mod error;
pub mod projects;
pub mod storage;
pub mod threads;

pub use collectors::{
    ChangedPath, DiffStats, FileDiffStat, GitSnapshot, ProjectBoundary, ResolvedPath,
    capture_snapshot, default_exclusions,
};
pub use domain::*;
pub use error::{DomainError, ErrorCode};
pub use projects::approve_project;
pub use storage::Database;
pub use threads::{
    LifecycleAction, PauseReason, RecoveryReport, ThreadService, recover_interrupted_threads,
};
