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

export type EventPayload = { "type": "thread_started", "payload": { goal: string, hypothesis: string | null, } } | { "type": "thread_paused", "payload": { reason: string, } } | { "type": "thread_resumed", "payload": { prior_checkpoint_id: CheckpointId | null, } } | { "type": "git_snapshot", "payload": JsonValue } | { "type": "file_changed", "payload": JsonValue } | { "type": "command_started", "payload": JsonValue } | { "type": "command_finished", "payload": JsonValue } | { "type": "note_added", "payload": { text: string, label: NoteLabel, } } | { "type": "checkpoint_requested", "payload": JsonValue } | { "type": "checkpoint_generated", "payload": JsonValue } | { "type": "checkpoint_approved", "payload": JsonValue } | { "type": "checkpoint_discarded", "payload": JsonValue };

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
binary: boolean, 
/**
 * Credentials replaced in this stream before it was stored.
 */
redactions: number, };

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

export type SourcedClaim = { id: string, claim: string, source_event_ids: Array<EventId>, };

export type Confidence = "low" | "medium" | "high";

export type Hypothesis = { id: string, claim: string, confidence: Confidence, source_event_ids: Array<EventId>, };

export type FailedAttempt = { action: string, outcome: string, learning: string, source_event_ids: Array<EventId>, };

export type RelevantArtifact = { type: string, relative_path: string, reason: string, source_event_ids: Array<EventId>, };

export type NextAction = { instruction: string, justification: string, success_check: string, source_event_ids: Array<EventId>, };

export type ResumeCapsule = { schema_version: string, goal: string, checkpoint_summary: string, verified_facts: Array<SourcedClaim>, hypotheses: Array<Hypothesis>, failed_attempts: Array<FailedAttempt>, unresolved_questions: Array<string>, relevant_artifacts: Array<RelevantArtifact>, next_action: NextAction, limitations: Array<string>, };

export type Checkpoint = { id: CheckpointId, threadId: ThreadId, startEventId: EventId, endEventId: EventId, status: string, modelId: string | null, promptVersion: string | null, payload: JsonValue, draft: ResumeCapsule | null, approved: ResumeCapsule | null, createdAt: string, approvedAt: string | null, };

export type Correction = { id: CorrectionId, checkpointId: CheckpointId, fieldPath: string, before: JsonValue, after: JsonValue, reason: string | null, createdAt: string, };

export type PreviewDisposition = "included" | "excluded" | "blocked" | "local_only";

export type PreviewFinding = { 
/**
 * What was found, in a sentence fragment such as "a bearer token".
 */
description: string, count: number, };

export type PayloadPreviewEntry = { eventId: EventId, timestamp: string, kind: string, disposition: PreviewDisposition, 
/**
 * Exactly what would be sent for an included entry. For a blocked entry
 * it is the text with the offending parts replaced, so a credential is
 * never shown again.
 */
value: JsonValue, 
/**
 * Size of `value` as sent.
 */
sizeBytes: number, reason: string | null, findings: Array<PreviewFinding>, };

export type PreviewCounts = { included: number, excluded: number, blocked: number, localOnly: number, };

export type PayloadPreview = { checkpointId: CheckpointId, threadTitle: string, goal: string, trigger: CheckpointTrigger, finalSnapshot: SnapshotStatus, entries: Array<PayloadPreviewEntry>, counts: PreviewCounts, 
/**
 * Changes made to the title, goal, or snapshot note, which cannot be
 * excluded.
 */
notices: Array<string>, 
/**
 * Size of everything that would be sent.
 */
totalSizeBytes: number, 
/**
 * The exact JSON document that would be sent, as serialized.
 */
payloadJson: string, 
/**
 * Identifies exactly what would be sent. Approval must present it back.
 */
payloadHash: string, hasBlockingFindings: boolean, 
/**
 * False while anything is blocked or nothing is selected.
 */
canGenerate: boolean, };

export type OutboundEntry = { eventId: EventId, timestamp: string, kind: string, content: JsonValue, };

export type OutboundInterval = { startEventId: EventId, endEventId: EventId, eventCount: number, };

export type OutboundPayload = { schemaVersion: number, threadTitle: string, goal: string, trigger: CheckpointTrigger, interval: OutboundInterval, finalSnapshot: SnapshotStatus, entries: Array<OutboundEntry>, omissions: Omissions, };

export type AppStatus = { service: string, version: string, ready: boolean, };
