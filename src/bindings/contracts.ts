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

export type WorkThread = { id: ThreadId, projectId: ProjectId, title: string, goal: string, state: ThreadState, startedAt: string, endedAt: string | null, createdAt: string, updatedAt: string, };

export type EventPayload = { "type": "thread_started", "payload": { goal: string, hypothesis: string | null, } } | { "type": "thread_paused", "payload": { reason: string, } } | { "type": "thread_resumed", "payload": { prior_checkpoint_id: CheckpointId | null, } } | { "type": "git_snapshot", "payload": JsonValue } | { "type": "file_changed", "payload": JsonValue } | { "type": "command_started", "payload": JsonValue } | { "type": "command_finished", "payload": JsonValue } | { "type": "note_added", "payload": { text: string, label: NoteLabel, } } | { "type": "checkpoint_requested", "payload": JsonValue } | { "type": "checkpoint_generated", "payload": JsonValue } | { "type": "checkpoint_approved", "payload": JsonValue };

export type ThreadEvent = { id: EventId, threadId: ThreadId, timestamp: string, payload: EventPayload, sensitivity: Sensitivity, contentHash: string, };

export type EventPage = { events: Array<ThreadEvent>, nextCursor: string | null, };

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
