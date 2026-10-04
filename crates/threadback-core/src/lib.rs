pub mod checkpoints;
pub mod collectors;
pub mod domain;
pub mod error;
pub mod inference;
pub mod outbound;
pub mod paths;
pub mod privacy;
pub mod projects;
pub mod storage;
pub mod threads;

pub use checkpoints::{
    CheckpointEntry, CheckpointPayload, CheckpointRequestedPayload, CheckpointService,
    CheckpointTrigger, Omissions, STATUS_DISCARDED, STATUS_DRAFT, SnapshotStatus,
};
pub use collectors::{
    CaptureLimits, ChangedPath, CommandFinishedPayload, CommandRequest, CommandStartedPayload,
    DiffStats, FileDiffStat, GitSnapshot, MAX_CAPTURE_BYTES, OutputCapture, OutputExcerpt,
    ProjectBoundary, ResolvedPath, RunIo, RunOutcome, capture_snapshot, default_exclusions,
    display_command, run_command,
};
pub use domain::*;
pub use error::{DomainError, ErrorCode};
pub use inference::{
    CapsuleModel, CapsuleSchema, ChatMessage, Evaluation, GeneratedCapsule, GenerationFailure,
    InvalidOutput, MAX_VIOLATIONS, ModelReply, PROMPT_VERSION, PromptRequest, SCHEMA_VERSION,
    SchemaIssue, ShownEntry, ValidationContext, Violation, ViolationKind, build_prompt,
    build_repair_prompt, capsule_schema, evaluate, generate_capsule, summarize, validate_capsule,
};
pub use outbound::{
    ApprovedPayload, OutboundEntry, OutboundInterval, OutboundPayload, OutboundService,
    PayloadPreview, PayloadPreviewEntry, PreviewCounts, PreviewFinding,
};
pub use paths::database_path;
pub use privacy::{Finding, FindingKind, Redaction, Redactor, StreamRedactor};
pub use projects::approve_project;
pub use storage::Database;
pub use threads::{
    LifecycleAction, PauseReason, RecoveryReport, ThreadService, recover_interrupted_threads,
};
