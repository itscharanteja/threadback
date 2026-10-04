use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        pub struct $name(pub String);

        impl $name {
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

id_type!(ProjectId);
id_type!(ThreadId);
id_type!(EventId);
id_type!(ArtifactId);
id_type!(CheckpointId);
id_type!(CorrectionId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ThreadState {
    Active,
    Paused,
    CheckpointDraft,
    Ready,
    Resumed,
    Archived,
    Failed,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NoteLabel {
    Fact,
    Hypothesis,
    Question,
    NextStep,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Normal,
    Sensitive,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: ProjectId,
    pub display_name: String,
    pub canonical_root: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectInput {
    pub display_name: String,
    pub root_path: String,
}

impl CreateProjectInput {
    pub fn validate(&self) -> Result<(), crate::DomainError> {
        if self.display_name.trim().is_empty() {
            return Err(crate::DomainError::invalid_input(
                "Project name is required.",
                "displayName",
            ));
        }
        if self.root_path.trim().is_empty() {
            return Err(crate::DomainError::invalid_input(
                "Project folder is required.",
                "rootPath",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPolicy {
    pub project_id: ProjectId,
    pub exclusions: Vec<String>,
    pub retention_days: u32,
    pub capture_git: bool,
    pub capture_commands: bool,
    pub capture_file_metadata: bool,
    pub redaction_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StartThreadInput {
    pub project_id: ProjectId,
    pub title: String,
    pub goal: String,
    pub hypothesis: Option<String>,
}

impl StartThreadInput {
    pub fn validate(&self) -> Result<(), crate::DomainError> {
        if self.title.trim().is_empty() {
            return Err(crate::DomainError::invalid_input(
                "A thread title is required.",
                "title",
            ));
        }
        if self.goal.trim().is_empty() {
            return Err(crate::DomainError::invalid_input(
                "A thread requires an intended outcome.",
                "goal",
            ));
        }
        Ok(())
    }
}

pub const MAX_NOTE_CHARS: usize = 8_000;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AddNoteInput {
    pub text: String,
    pub label: NoteLabel,
}

impl AddNoteInput {
    /// The text is stored exactly as typed; it is only rejected when blank or
    /// too long.
    pub fn validate(&self) -> Result<(), crate::DomainError> {
        if self.text.trim().is_empty() {
            return Err(crate::DomainError::invalid_input(
                "A note cannot be empty.",
                "text",
            ));
        }
        if self.text.chars().count() > MAX_NOTE_CHARS {
            return Err(crate::DomainError::invalid_input(
                format!("A note can be at most {MAX_NOTE_CHARS} characters."),
                "text",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkThread {
    pub id: ThreadId,
    pub project_id: ProjectId,
    pub title: String,
    pub goal: String,
    pub state: ThreadState,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum EventPayload {
    ThreadStarted {
        goal: String,
        hypothesis: Option<String>,
    },
    ThreadPaused {
        reason: String,
    },
    ThreadResumed {
        prior_checkpoint_id: Option<CheckpointId>,
    },
    GitSnapshot(Value),
    FileChanged(Value),
    CommandStarted(Value),
    CommandFinished(Value),
    NoteAdded {
        text: String,
        label: NoteLabel,
    },
    CheckpointRequested(Value),
    CheckpointGenerated(Value),
    CheckpointApproved(Value),
    CheckpointDiscarded(Value),
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEvent {
    pub id: EventId,
    pub thread_id: ThreadId,
    pub timestamp: DateTime<Utc>,
    pub payload: EventPayload,
    pub sensitivity: Sensitivity,
    pub content_hash: String,
}

impl ThreadEvent {
    /// Builds an event stamped now. Credentials in the payload are replaced
    /// before the content hash is computed, so neither the stored text nor
    /// its hash depends on a secret.
    pub fn new(
        thread_id: ThreadId,
        payload: EventPayload,
        sensitivity: Sensitivity,
    ) -> Result<Self, crate::DomainError> {
        let content_hash = hash_payload(&payload)?;
        let event = Self {
            id: EventId::new(),
            thread_id,
            timestamp: Utc::now(),
            payload,
            sensitivity,
            content_hash,
        };
        Ok(event.redacted()?.unwrap_or(event))
    }

    /// The same event with credentials and home paths replaced, or `None`
    /// when there was nothing to replace. An event that had a credential is
    /// marked `Sensitive`. The storage layer applies this to every event it
    /// writes, whatever built the event.
    pub(crate) fn redacted(&self) -> Result<Option<Self>, crate::DomainError> {
        let Some((payload, secrets)) = crate::privacy::sanitize_payload(&self.payload)? else {
            return Ok(None);
        };
        let content_hash = hash_payload(&payload)?;
        let sensitivity = if secrets > 0 && self.sensitivity == Sensitivity::Normal {
            Sensitivity::Sensitive
        } else {
            self.sensitivity.clone()
        };
        Ok(Some(Self {
            id: self.id.clone(),
            thread_id: self.thread_id.clone(),
            timestamp: self.timestamp,
            payload,
            sensitivity,
            content_hash,
        }))
    }
}

pub(crate) fn hash_payload(payload: &EventPayload) -> Result<String, crate::DomainError> {
    use sha2::{Digest, Sha256};

    let serialized = serde_json::to_vec(payload).map_err(|error| {
        crate::DomainError::internal(format!("Local JSON serialization failed: {error}"))
    })?;
    Ok(Sha256::digest(&serialized)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EventPage {
    pub events: Vec<ThreadEvent>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRef {
    pub id: ArtifactId,
    pub event_id: EventId,
    pub artifact_type: String,
    pub relative_path: String,
    pub content_hash: String,
    pub line_hint: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SourcedClaim {
    pub id: String,
    pub claim: String,
    pub source_event_ids: Vec<EventId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Hypothesis {
    pub id: String,
    pub claim: String,
    pub confidence: Confidence,
    pub source_event_ids: Vec<EventId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FailedAttempt {
    pub action: String,
    pub outcome: String,
    pub learning: String,
    pub source_event_ids: Vec<EventId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RelevantArtifact {
    pub artifact_type: String,
    pub relative_path: String,
    pub reason: String,
    pub source_event_ids: Vec<EventId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NextAction {
    pub instruction: String,
    pub justification: String,
    pub success_check: String,
    pub source_event_ids: Vec<EventId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResumeCapsule {
    pub schema_version: String,
    pub goal: String,
    pub checkpoint_summary: String,
    pub verified_facts: Vec<SourcedClaim>,
    pub hypotheses: Vec<Hypothesis>,
    pub failed_attempts: Vec<FailedAttempt>,
    pub unresolved_questions: Vec<String>,
    pub relevant_artifacts: Vec<RelevantArtifact>,
    pub next_action: NextAction,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: CheckpointId,
    pub thread_id: ThreadId,
    pub start_event_id: EventId,
    pub end_event_id: EventId,
    pub status: String,
    pub model_id: Option<String>,
    pub prompt_version: Option<String>,
    pub payload: Value,
    pub draft: Option<ResumeCapsule>,
    pub approved: Option<ResumeCapsule>,
    pub created_at: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Correction {
    pub id: CorrectionId,
    pub checkpoint_id: CheckpointId,
    pub field_path: String,
    pub before: Value,
    pub after: Value,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PreviewDisposition {
    Included,
    Excluded,
    Blocked,
    LocalOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub service: String,
    pub version: String,
    pub ready: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_state_uses_stable_wire_value() {
        assert_eq!(
            serde_json::to_string(&ThreadState::Active).unwrap(),
            "\"ACTIVE\""
        );
    }

    #[test]
    fn note_event_is_discriminated() {
        let payload = EventPayload::NoteAdded {
            text: "The test fails".into(),
            label: NoteLabel::Fact,
        };
        let value = serde_json::to_value(payload).unwrap();
        assert_eq!(value["type"], "note_added");
        assert_eq!(value["payload"]["label"], "fact");
    }

    #[test]
    fn project_input_rejects_empty_fields() {
        let input = CreateProjectInput {
            display_name: " ".into(),
            root_path: "".into(),
        };
        let error = input.validate().unwrap_err();
        assert_eq!(error.code, crate::ErrorCode::InvalidInput);
        assert_eq!(error.field.as_deref(), Some("displayName"));
    }
}
