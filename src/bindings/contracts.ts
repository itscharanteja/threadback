// This file is generated from Rust. Run `npm run bindings` after contract changes.

export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export type ProjectId = string;

export type ThreadId = string;

export type EventId = string;

export type ArtifactId = string;

export type CheckpointId = string;

export type CorrectionId = string;

export type ThreadState = "ACTIVE" | "PAUSED" | "CHECKPOINT_DRAFT" | "READY" | "RESUMED" | "ARCHIVED" | "FAILED" | "DELETED";

export type NoteLabel = "fact" | "hypothesis" | "question" | "next_step";

export type Sensitivity = "normal" | "sensitive" | "blocked";

export type Project = { id: ProjectId, displayName: string, canonicalRoot: string, createdAt: string, updatedAt: string, };

export type CreateProjectInput = { displayName: string, rootPath: string, };

export type ProjectPolicy = { projectId: ProjectId, exclusions: Array<string>, retentionDays: number, captureGit: boolean, captureCommands: boolean, captureFileMetadata: boolean, redactionPatterns: Array<string>, };

export type StartThreadInput = { projectId: ProjectId, title: string, goal: string, hypothesis: string | null, };

export type AddNoteInput = { text: string, label: NoteLabel, };

export type WorkThread = { id: ThreadId, projectId: ProjectId, title: string, goal: string, state: ThreadState, startedAt: string, endedAt: string | null, createdAt: string, updatedAt: string, };

export type EventPayload = { "type": "thread_started", "payload": { goal: string, hypothesis: string | null, } } | { "type": "thread_paused", "payload": { reason: string, } } | { "type": "thread_resumed", "payload": { prior_checkpoint_id: CheckpointId | null, } } | { "type": "git_snapshot", "payload": JsonValue } | { "type": "file_changed", "payload": JsonValue } | { "type": "command_started", "payload": JsonValue } | { "type": "command_finished", "payload": JsonValue } | { "type": "note_added", "payload": { text: string, label: NoteLabel, } } | { "type": "checkpoint_requested", "payload": JsonValue } | { "type": "checkpoint_generated", "payload": JsonValue } | { "type": "checkpoint_approved", "payload": JsonValue };

export type ThreadEvent = { id: EventId, threadId: ThreadId, timestamp: string, payload: EventPayload, sensitivity: Sensitivity, contentHash: string, };

export type EventPage = { events: Array<ThreadEvent>, nextCursor: string | null, };

export type ChangedPath = { path: string, 
/**
 * Porcelain index column, e.g. `M`, `A`, `D`, `?`.
 */
indexStatus: string, 
/**
 * Porcelain worktree column.
 */
worktreeStatus: string, };

export type FileDiffStat = { path: string, additions: number, deletions: number, binary: boolean, };

export type DiffStats = { files: Array<FileDiffStat>, fileCount: number, additions: number, deletions: number, };

export type GitSnapshot = { 
/**
 * `None` when HEAD is detached.
 */
branch: string | null, 
/**
 * `None` when the repository has no commits yet.
 */
head: string | null, 
/**
 * True when Git reports any change, including excluded paths.
 */
dirty: boolean, changedPaths: Array<ChangedPath>, staged: DiffStats, unstaged: DiffStats, 
/**
 * Changed paths withheld because they are excluded or resolve outside the
 * project.
 */
excludedChanges: number, 
/**
 * True when a path list was cut at the listing limit.
 */
truncated: boolean, };

export type CommandStartedPayload = { 
/**
 * Display form of the command line. Never includes the environment.
 */
command: string, 
/**
 * Working directory relative to the project root.
 */
cwd: string, };

export type OutputExcerpt = { head: string, tail: string, originalBytes: number, omittedBytes: number, truncated: boolean, 
/**
 * Binary output is counted but never stored.
 */
binary: boolean, };

export type CommandFinishedPayload = { startedEventId: EventId, 
/**
 * `None` when the process was ended by a signal.
 */
exitCode: number | null, signal: number | null, durationMs: number, stdout: OutputExcerpt, stderr: OutputExcerpt, };

export type CheckpointTrigger = "manual" | "idle_suggestion" | "thread_ended";

export type SnapshotStatus = { captured: boolean, eventId: EventId | null, 
/**
 * Why no snapshot was taken, in words the user can read.
 */
reason: string | null, };

export type CheckpointEntry = { eventId: EventId, timestamp: string, 
/**
 * The event type, such as `note.added` or `command.finished`.
 */
kind: string, content: JsonValue, };

export type Omissions = { duplicatesRemoved: number, overBudgetRemoved: number, notesShortened: number, unreadable: number, };

export type CheckpointPayload = { version: number, threadTitle: string, goal: string, trigger: CheckpointTrigger, startEventId: EventId, endEventId: EventId, 
/**
 * Events in the interval before selection.
 */
intervalEventCount: number, finalSnapshot: SnapshotStatus, entries: Array<CheckpointEntry>, omissions: Omissions, totalChars: number, };

export type CheckpointRequestedPayload = { checkpointId: CheckpointId, trigger: CheckpointTrigger, startEventId: EventId, endEventId: EventId, };

export type RecoveryReport = { threadId: ThreadId, threadTitle: string, 
/**
 * Timestamp of the last acknowledged event, which starts the gap.
 */
lastEventAt: string | null, recoveredAt: string, };

export type ArtifactRef = { id: ArtifactId, eventId: EventId, artifactType: string, relativePath: string, contentHash: string, lineHint: number | null, };

export type SourcedClaim = { id: string, claim: string, sourceEventIds: Array<EventId>, };

export type Confidence = "low" | "medium" | "high";

export type Hypothesis = { id: string, claim: string, confidence: Confidence, sourceEventIds: Array<EventId>, };

export type FailedAttempt = { action: string, outcome: string, learning: string, sourceEventIds: Array<EventId>, };

export type RelevantArtifact = { artifactType: string, relativePath: string, reason: string, sourceEventIds: Array<EventId>, };

export type NextAction = { instruction: string, justification: string, successCheck: string, sourceEventIds: Array<EventId>, };

export type ResumeCapsule = { schemaVersion: string, goal: string, checkpointSummary: string, verifiedFacts: Array<SourcedClaim>, hypotheses: Array<Hypothesis>, failedAttempts: Array<FailedAttempt>, unresolvedQuestions: Array<string>, relevantArtifacts: Array<RelevantArtifact>, nextAction: NextAction, limitations: Array<string>, };

export type Checkpoint = { id: CheckpointId, threadId: ThreadId, startEventId: EventId, endEventId: EventId, status: string, modelId: string | null, promptVersion: string | null, payload: JsonValue, draft: ResumeCapsule | null, approved: ResumeCapsule | null, createdAt: string, approvedAt: string | null, };

export type Correction = { id: CorrectionId, checkpointId: CheckpointId, fieldPath: string, before: JsonValue, after: JsonValue, reason: string | null, createdAt: string, };

export type PreviewDisposition = "included" | "excluded" | "blocked" | "local_only";

export type PayloadPreviewEntry = { eventId: EventId, disposition: PreviewDisposition, value: JsonValue, sizeBytes: number, reason: string | null, };

export type PayloadPreview = { checkpointId: CheckpointId, entries: Array<PayloadPreviewEntry>, totalSizeBytes: number, hasBlockingFindings: boolean, };

export type AppStatus = { service: string, version: string, ready: boolean, };
