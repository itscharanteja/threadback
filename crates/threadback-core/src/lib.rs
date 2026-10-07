pub mod approval;
pub mod checkpoints;
pub mod collectors;
pub mod config;
pub mod domain;
pub mod error;
pub mod evaluation;
pub mod export;
pub mod inference;
pub mod outbound;
pub mod paths;
pub mod privacy;
pub mod projects;
pub mod storage;
pub mod threads;

pub use approval::{ApprovalResult, ApprovalService, FieldChange, diff_capsules};
pub use checkpoints::{
    CheckpointEntry, CheckpointPayload, CheckpointRequestedPayload, CheckpointService,
    CheckpointTrigger, DriftKind, DriftSignal, Omissions, OpenedArtifact, RepositoryDrift,
    ResumeViewModel, STATUS_APPROVED, STATUS_DISCARDED, STATUS_DRAFT, STATUS_GENERATED,
    SnapshotStatus, build_resume_view, compare_snapshots, open_checkpoint_artifact,
};
pub use collectors::{
    CaptureLimits, ChangedPath, CommandFinishedPayload, CommandRequest, CommandStartedPayload,
    DiffStats, FileDiffStat, GitSnapshot, MAX_CAPTURE_BYTES, OutputCapture, OutputExcerpt,
    ProjectBoundary, ResolvedPath, RunIo, RunOutcome, capture_snapshot, default_exclusions,
    display_command, run_command,
};
pub use config::{
    ApiKey, ConfigurationService, ConfigurationStatus, ConnectionTestResult, MemorySecretStore,
    ModelConfigInput, ModelSettings, SecretStore, StructuredOutput, default_secret_store,
};
pub use domain::*;
pub use error::{DomainError, ErrorCode};
pub use evaluation::{
    EvaluationReport, EvaluationSummary, MetricComparison, ScenarioOutcome, ScenarioSpec,
    load_scenario_specs, render_evaluation_markdown, run_evaluation_suite,
};
pub use export::{ExportService, FileResult, render_markdown};
#[cfg(feature = "model-client")]
pub use inference::{CancelToken, TokenFactoryClient};
pub use inference::{
    CapsuleModel, CapsuleSchema, ChatMessage, Evaluation, GeneratedCapsule, GenerationFailure,
    GenerationPlan, InvalidOutput, MAX_VIOLATIONS, ModelCallRecord, ModelReply, PROMPT_VERSION,
    PromptRequest, SCHEMA_VERSION, SchemaIssue, ShownEntry, ValidationContext, Violation,
    ViolationKind, build_prompt, build_repair_prompt, capsule_schema, check_capsule, evaluate,
    finish_generation, generate_capsule, generate_draft, plan_generation, run_generation,
    summarize, validate_capsule,
};
pub use outbound::{
    ApprovedPayload, OutboundEntry, OutboundInterval, OutboundPayload, OutboundService,
    PayloadPreview, PayloadPreviewEntry, PreviewCounts, PreviewFinding,
};
pub use paths::database_path;
pub use privacy::{Finding, FindingKind, Redaction, Redactor, StreamRedactor, test_redaction};
pub use projects::{approve_project, relink_project};
pub use storage::{Database, LatencySummary};
pub use threads::{
    LifecycleAction, PauseReason, RecoveryReport, ThreadService, recover_interrupted_threads,
};
