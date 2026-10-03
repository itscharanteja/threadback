pub mod checkpoints;
pub mod collectors;
pub mod domain;
pub mod error;
pub mod paths;
pub mod projects;
pub mod storage;
pub mod threads;

pub use checkpoints::{
    CheckpointEntry, CheckpointPayload, CheckpointRequestedPayload, CheckpointService,
    CheckpointTrigger, Omissions, STATUS_DRAFT, SnapshotStatus,
};
pub use collectors::{
    CaptureLimits, ChangedPath, CommandFinishedPayload, CommandRequest, CommandStartedPayload,
    DiffStats, FileDiffStat, GitSnapshot, MAX_CAPTURE_BYTES, OutputCapture, OutputExcerpt,
    ProjectBoundary, ResolvedPath, RunIo, RunOutcome, capture_snapshot, default_exclusions,
    display_command, run_command,
};
pub use domain::*;
pub use error::{DomainError, ErrorCode};
pub use paths::database_path;
pub use projects::approve_project;
pub use storage::Database;
pub use threads::{
    LifecycleAction, PauseReason, RecoveryReport, ThreadService, recover_interrupted_threads,
};
