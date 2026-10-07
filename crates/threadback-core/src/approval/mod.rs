mod diff;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::{
    Checkpoint, CheckpointId, CheckpointPayload, Correction, CorrectionId, Database, DomainError,
    EventPayload, ProjectBoundary, ResumeCapsule, STATUS_APPROVED, STATUS_GENERATED, ShownEntry,
    ValidationContext, WorkThread,
    inference::validate::{check_capsule, summarize},
    privacy::{Redactor, redact_value},
    threads::{
        lifecycle::LifecycleAction,
        service::{apply_transition, require_thread},
    },
};

pub use diff::{FieldChange, diff_capsules};

/// What approving a capsule produced.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalResult {
    /// The checkpoint with both versions: `draft` is what the model returned,
    /// untouched; `approved` is what the user signed off on.
    pub checkpoint: Checkpoint,
    /// `READY`.
    pub thread: WorkThread,
    /// Each change the user made to the draft, by field path. Empty when the
    /// draft was approved as it stood.
    pub corrections: Vec<Correction>,
}

pub struct ApprovalService<'a> {
    database: &'a Database,
}

impl<'a> ApprovalService<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    /// Approves `capsule` as the checkpoint's capsule. It is the draft with
    /// whatever the user edited, relabelled, or removed; pass the draft itself
    /// to approve it as it stands.
    ///
    /// The capsule must pass the checks the model's reply passed, so an edit
    /// cannot introduce a verified fact that rests on nothing, cite an event
    /// outside the checkpoint, or carry a credential. An edit may cite any
    /// entry of the checkpoint, including ones kept from the model: that
    /// evidence is on this machine and the person editing is the authority.
    ///
    /// The approved capsule, the corrections, the `checkpoint.approved` event,
    /// and the thread's move to `READY` are committed together. The model's
    /// draft is never written to.
    pub fn approve(
        &self,
        checkpoint_id: &CheckpointId,
        capsule: ResumeCapsule,
    ) -> Result<ApprovalResult, DomainError> {
        let checkpoint = self
            .database
            .find_checkpoint(checkpoint_id)?
            .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
        let draft = match (checkpoint.status.as_str(), &checkpoint.draft) {
            (STATUS_APPROVED, _) => {
                return Err(DomainError::invalid_state(
                    "This capsule was already approved.",
                ));
            }
            (STATUS_GENERATED, Some(draft)) => draft.clone(),
            _ => {
                return Err(DomainError::invalid_state(
                    "There is no generated capsule to approve yet.",
                ));
            }
        };

        let context = self.context(&checkpoint)?;
        let violations = check_capsule(&capsule, &context);
        if let Some(first) = violations.first() {
            return Err(DomainError::invalid_capsule(
                format!(
                    "This capsule cannot be approved yet. {}",
                    summarize(&violations, 5)
                ),
                (!first.path.is_empty()).then(|| first.path.clone()),
            ));
        }

        // What is stored and compared is the redacted capsule, so a home path
        // in an edit is recorded the way it is everywhere else.
        let approved = redacted(&capsule)?;
        let changes = diff_capsules(&draft, &approved)?;
        let approved_hash = fingerprint(&approved)?;

        self.database.with_transaction(|db| {
            let now = Utc::now();
            db.set_checkpoint_approved(checkpoint_id, &approved, now)?;

            let corrections: Vec<Correction> = changes
                .into_iter()
                .map(|change| Correction {
                    id: CorrectionId::new(),
                    checkpoint_id: checkpoint_id.clone(),
                    field_path: change.field_path,
                    before: change.before,
                    after: change.after,
                    reason: None,
                    created_at: now,
                })
                .collect();
            for correction in &corrections {
                db.insert_correction(correction)?;
            }

            let count = corrections.len();
            let thread = apply_transition(
                db,
                &checkpoint.thread_id,
                LifecycleAction::MarkReady,
                |_| {
                    Ok(Some(EventPayload::CheckpointApproved(json!({
                        "checkpointId": checkpoint_id,
                        "correctionCount": count,
                        "approvedHash": approved_hash,
                    }))))
                },
            )?;

            let checkpoint = db
                .find_checkpoint(checkpoint_id)?
                .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
            Ok(ApprovalResult {
                checkpoint,
                thread,
                corrections,
            })
        })
    }

    /// Every correction made when this checkpoint was approved, oldest first.
    pub fn corrections(
        &self,
        checkpoint_id: &CheckpointId,
    ) -> Result<Vec<Correction>, DomainError> {
        self.database
            .find_checkpoint(checkpoint_id)?
            .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
        self.database.list_corrections(checkpoint_id)
    }

    /// An edited capsule may cite any entry of the checkpoint.
    fn context(&self, checkpoint: &Checkpoint) -> Result<ValidationContext, DomainError> {
        let payload: CheckpointPayload = serde_json::from_value(checkpoint.payload.clone())
            .map_err(|_| DomainError::internal("The checkpoint draft could not be read."))?;
        let thread = require_thread(self.database, &checkpoint.thread_id)?;
        let project = self
            .database
            .find_project(&thread.project_id)?
            .ok_or_else(|| DomainError::not_found("The thread's project is no longer approved."))?;
        let policy = self
            .database
            .find_project_policy(&project.id)
            .unwrap_or_else(|_| crate::ProjectPolicy::default_for(project.id.clone()));
        let boundary = ProjectBoundary::new(&project.canonical_root, &policy.exclusions)?;
        let entries = payload.entries.iter().map(|entry| {
            ShownEntry::from_content(entry.event_id.clone(), &entry.kind, &entry.content)
        });
        Ok(ValidationContext::new(entries, [], boundary))
    }
}

fn redacted(capsule: &ResumeCapsule) -> Result<ResumeCapsule, DomainError> {
    let internal = |error: serde_json::Error| {
        DomainError::internal(format!("Capsule serialization failed: {error}"))
    };
    let mut value = serde_json::to_value(capsule).map_err(internal)?;
    redact_value(Redactor::builtin(), &mut value);
    serde_json::from_value(value).map_err(internal)
}

/// A fingerprint of the approved capsule, recorded with the approval event so
/// an export can be shown to match what was approved.
fn fingerprint(capsule: &ResumeCapsule) -> Result<String, DomainError> {
    let bytes = serde_json::to_vec(capsule)
        .map_err(|error| DomainError::internal(format!("Capsule serialization failed: {error}")))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
