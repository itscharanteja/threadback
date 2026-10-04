mod select;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::{
    Checkpoint, CheckpointId, Database, DomainError, EventId, EventPayload, ThreadId,
    ThreadService, WorkThread,
    threads::{
        lifecycle::{LifecycleAction, next_state},
        service::{apply_transition, require_thread},
    },
};

pub use select::{SelectionLimits, build_payload};

/// A checkpoint that exists only locally and has not been sent to a model.
pub const STATUS_DRAFT: &str = "draft";

/// The model has produced a capsule draft. It is kept exactly as returned.
pub const STATUS_GENERATED: &str = "generated";

/// A draft the user threw away. Its evidence is not consumed: the next
/// checkpoint covers it again.
pub const STATUS_DISCARDED: &str = "discarded";

/// Why a checkpoint was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointTrigger {
    /// The user pressed Save My Place.
    Manual,
    /// The user accepted an inactivity suggestion.
    IdleSuggestion,
    /// The user ended the thread.
    ThreadEnded,
}

/// Whether a final Git snapshot was taken when the checkpoint was saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotStatus {
    pub captured: bool,
    pub event_id: Option<EventId>,
    /// Why no snapshot was taken, in words the user can read.
    pub reason: Option<String>,
}

/// One piece of evidence, condensed for the checkpoint. `eventId` points at
/// the full local event, so every claim built on it stays traceable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointEntry {
    pub event_id: EventId,
    pub timestamp: DateTime<Utc>,
    /// The event type, such as `note.added` or `command.finished`.
    pub kind: String,
    pub content: Value,
}

/// What was left out of the checkpoint, so nothing disappears silently.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Omissions {
    pub duplicates_removed: u32,
    pub over_budget_removed: u32,
    pub notes_shortened: u32,
    pub unreadable: u32,
}

/// The local content of a checkpoint draft, stored as the checkpoint payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointPayload {
    pub version: u32,
    pub thread_title: String,
    pub goal: String,
    pub trigger: CheckpointTrigger,
    pub start_event_id: EventId,
    pub end_event_id: EventId,
    /// Events in the interval before selection.
    pub interval_event_count: u32,
    pub final_snapshot: SnapshotStatus,
    pub entries: Vec<CheckpointEntry>,
    pub omissions: Omissions,
    pub total_chars: u32,
}

/// Payload of a `checkpoint.requested` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointRequestedPayload {
    pub checkpoint_id: CheckpointId,
    pub trigger: CheckpointTrigger,
    pub start_event_id: EventId,
    pub end_event_id: EventId,
}

pub struct CheckpointService<'a> {
    database: &'a Database,
}

impl<'a> CheckpointService<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    /// Save My Place. Needs no network: it takes a final Git snapshot when it
    /// can, selects the evidence since the previous checkpoint, and stores a
    /// draft. The draft, the `checkpoint.requested` event, and the thread's
    /// move to `CHECKPOINT_DRAFT` are committed together.
    pub fn save(
        &self,
        thread_id: &ThreadId,
        trigger: CheckpointTrigger,
    ) -> Result<Checkpoint, DomainError> {
        let action = match trigger {
            CheckpointTrigger::ThreadEnded => LifecycleAction::End,
            CheckpointTrigger::Manual | CheckpointTrigger::IdleSuggestion => {
                LifecycleAction::BeginCheckpoint
            }
        };

        // Reject an impossible request before doing any work.
        let thread = require_thread(self.database, thread_id)?;
        next_state(&thread.state, action)?;

        // A failed snapshot must not lose the user's place: the checkpoint is
        // still saved, and says why there is no final snapshot.
        let final_snapshot = if thread.state.accepts_evidence() {
            match ThreadService::new(self.database).capture_git(thread_id) {
                Ok(event) => SnapshotStatus {
                    captured: true,
                    event_id: Some(event.id),
                    reason: None,
                },
                Err(error) => SnapshotStatus {
                    captured: false,
                    event_id: None,
                    reason: Some(error.message),
                },
            }
        } else {
            SnapshotStatus {
                captured: false,
                event_id: None,
                reason: Some("The thread was paused, so no new snapshot was taken.".into()),
            }
        };

        self.database.with_transaction(|db| {
            let thread = require_thread(db, thread_id)?;
            next_state(&thread.state, action)?;

            let events = db.list_all_events(&thread.id)?;
            // A discarded draft consumed nothing, so it does not move the start.
            let previous_end = db
                .list_checkpoints(&thread.id)?
                .into_iter()
                .find(|checkpoint| checkpoint.status != STATUS_DISCARDED)
                .map(|checkpoint| checkpoint.end_event_id);
            let start = previous_end
                .and_then(|end| events.iter().position(|event| event.id == end))
                .map_or(0, |index| index + 1);
            let interval = &events[start..];
            let (first, last) = match (interval.first(), interval.last()) {
                (Some(first), Some(last)) => (first, last),
                _ => {
                    return Err(DomainError::invalid_state(
                        "There is no new evidence since the last checkpoint.",
                    ));
                }
            };

            let payload = build_payload(
                &thread,
                interval,
                trigger,
                final_snapshot.clone(),
                &SelectionLimits::default(),
            );
            let checkpoint = Checkpoint {
                id: CheckpointId::new(),
                thread_id: thread.id.clone(),
                start_event_id: first.id.clone(),
                end_event_id: last.id.clone(),
                status: STATUS_DRAFT.into(),
                model_id: None,
                prompt_version: None,
                payload: serde_json::to_value(&payload).map_err(|error| {
                    DomainError::internal(format!("Checkpoint serialization failed: {error}"))
                })?,
                draft: None,
                approved: None,
                created_at: Utc::now(),
                approved_at: None,
            };
            db.insert_checkpoint(&checkpoint)?;

            let requested = CheckpointRequestedPayload {
                checkpoint_id: checkpoint.id.clone(),
                trigger,
                start_event_id: checkpoint.start_event_id.clone(),
                end_event_id: checkpoint.end_event_id.clone(),
            };
            apply_transition(db, &thread.id, action, |_| {
                Ok(Some(EventPayload::CheckpointRequested(
                    serde_json::to_value(&requested).map_err(|error| {
                        DomainError::internal(format!("Event serialization failed: {error}"))
                    })?,
                )))
            })?;
            Ok(checkpoint)
        })
    }

    /// Throws a draft away and returns the thread to recording. Nothing was
    /// sent, so nothing needs undoing. Refused when the thread cannot record
    /// again, for instance because another thread is already recording.
    pub fn discard(&self, id: &CheckpointId) -> Result<WorkThread, DomainError> {
        self.database.with_transaction(|db| {
            let checkpoint = db
                .find_checkpoint(id)?
                .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
            if checkpoint.status != STATUS_DRAFT && checkpoint.status != STATUS_GENERATED {
                return Err(DomainError::invalid_state(
                    "Only a draft that has not been approved can be discarded.",
                ));
            }
            // Check the thread can resume before marking anything.
            let thread =
                apply_transition(db, &checkpoint.thread_id, LifecycleAction::Discard, |_| {
                    Ok(Some(EventPayload::CheckpointDiscarded(serde_json::json!({
                        "checkpointId": checkpoint.id,
                    }))))
                })?;
            db.update_checkpoint_status(id, STATUS_DISCARDED)?;
            Ok(thread)
        })
    }

    pub fn get(&self, id: &CheckpointId) -> Result<Checkpoint, DomainError> {
        self.database
            .find_checkpoint(id)?
            .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))
    }

    /// Newest first.
    pub fn list(&self, thread_id: &ThreadId) -> Result<Vec<Checkpoint>, DomainError> {
        require_thread(self.database, thread_id)?;
        self.database.list_checkpoints(thread_id)
    }
}
