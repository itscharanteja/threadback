use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    CheckpointId, Confidence, Database, DomainError, ResumeCapsule, STATUS_APPROVED,
    paths::thread_exports_directory,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileResult {
    pub path: String,
    pub format: String,
    pub bytes: u64,
}

/// Exports only an approved capsule. Unapproved drafts and excluded evidence
/// can never be exported.
pub struct ExportService<'a> {
    database: &'a Database,
}

impl<'a> ExportService<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    pub fn export_json(&self, checkpoint_id: &CheckpointId) -> Result<FileResult, DomainError> {
        let (thread, checkpoint, approved) = self.require_approved(checkpoint_id)?;
        let content = serde_json::to_string_pretty(&approved).map_err(|error| {
            DomainError::internal(format!("Could not serialize approved capsule: {error}"))
        })?;
        let path = write_export(
            &thread_exports_directory(&thread.id),
            &format!("capsule-{}.json", checkpoint.id.0),
            &content,
        )?;
        Ok(FileResult {
            path: path.to_string_lossy().into_owned(),
            format: "json".into(),
            bytes: content.len() as u64,
        })
    }

    pub fn export_markdown(&self, checkpoint_id: &CheckpointId) -> Result<FileResult, DomainError> {
        let (thread, checkpoint, approved) = self.require_approved(checkpoint_id)?;
        let content = render_markdown(&thread.title, &checkpoint.id, &approved);
        let path = write_export(
            &thread_exports_directory(&thread.id),
            &format!("capsule-{}.md", checkpoint.id.0),
            &content,
        )?;
        Ok(FileResult {
            path: path.to_string_lossy().into_owned(),
            format: "markdown".into(),
            bytes: content.len() as u64,
        })
    }

    fn require_approved(
        &self,
        checkpoint_id: &CheckpointId,
    ) -> Result<(crate::WorkThread, crate::Checkpoint, ResumeCapsule), DomainError> {
        let checkpoint = self
            .database
            .find_checkpoint(checkpoint_id)?
            .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
        if checkpoint.status != STATUS_APPROVED {
            return Err(DomainError::invalid_state(
                "Only an approved capsule can be exported.",
            ));
        }
        let approved = checkpoint.approved.clone().ok_or_else(|| {
            DomainError::invalid_state("Only an approved capsule can be exported.")
        })?;
        let thread = self
            .database
            .find_thread(&checkpoint.thread_id)?
            .ok_or_else(|| DomainError::not_found("That thread no longer exists."))?;
        Ok((thread, checkpoint, approved))
    }
}

fn write_export(dir: &std::path::Path, filename: &str, content: &str) -> Result<PathBuf, DomainError> {
    std::fs::create_dir_all(dir).map_err(|error| {
        DomainError::storage(format!("Could not create export directory: {error}"))
    })?;
    let path = dir.join(filename);
    std::fs::write(&path, content).map_err(|error| {
        DomainError::storage(format!("Could not write export file: {error}"))
    })?;
    Ok(path)
}

pub fn render_markdown(
    thread_title: &str,
    checkpoint_id: &CheckpointId,
    capsule: &ResumeCapsule,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", thread_title.trim()));
    out.push_str(&format!(
        "- **Schema version**: `{}`\n- **Checkpoint ID**: `{}`\n- **Goal**: {}\n\n",
        capsule.schema_version,
        checkpoint_id.0,
        capsule.goal.trim()
    ));

    out.push_str("## Next Action\n\n");
    out.push_str(&format!("**{}**\n\n", capsule.next_action.instruction.trim()));
    out.push_str(&format!(
        "- **Justification**: {}\n- **Success check**: {}\n- **Sources**: {}\n\n",
        capsule.next_action.justification.trim(),
        capsule.next_action.success_check.trim(),
        format_sources(&capsule.next_action.source_event_ids)
    ));

    out.push_str("## Summary\n\n");
    out.push_str(&format!("{}\n\n", capsule.checkpoint_summary.trim()));

    out.push_str("## Verified Facts\n\n");
    if capsule.verified_facts.is_empty() {
        out.push_str("_None recorded._\n\n");
    } else {
        for fact in &capsule.verified_facts {
            out.push_str(&format!(
                "- **[FACT]** `{}`: {} _(sources: {})_\n",
                fact.id,
                fact.claim.trim(),
                format_sources(&fact.source_event_ids)
            ));
        }
        out.push('\n');
    }

    out.push_str("## Hypotheses\n\n");
    if capsule.hypotheses.is_empty() {
        out.push_str("_None recorded._\n\n");
    } else {
        for hyp in &capsule.hypotheses {
            let conf = match hyp.confidence {
                Confidence::Low => "low",
                Confidence::Medium => "medium",
                Confidence::High => "high",
            };
            out.push_str(&format!(
                "- **[HYPOTHESIS · {conf}]** `{}`: {} _(sources: {})_\n",
                hyp.id,
                hyp.claim.trim(),
                format_sources(&hyp.source_event_ids)
            ));
        }
        out.push('\n');
    }

    out.push_str("## Failed Attempts\n\n");
    if capsule.failed_attempts.is_empty() {
        out.push_str("_None recorded._\n\n");
    } else {
        for attempt in &capsule.failed_attempts {
            out.push_str(&format!(
                "- **Action**: {}\n  - **Outcome**: {}\n  - **Learning**: {}\n  - **Sources**: {}\n",
                attempt.action.trim(),
                attempt.outcome.trim(),
                attempt.learning.trim(),
                format_sources(&attempt.source_event_ids)
            ));
        }
        out.push('\n');
    }

    out.push_str("## Unresolved Questions\n\n");
    if capsule.unresolved_questions.is_empty() {
        out.push_str("_None recorded._\n\n");
    } else {
        for question in &capsule.unresolved_questions {
            out.push_str(&format!("- {}\n", question.trim()));
        }
        out.push('\n');
    }

    out.push_str("## Files That Matter\n\n");
    if capsule.relevant_artifacts.is_empty() {
        out.push_str("_None recorded._\n\n");
    } else {
        for artifact in &capsule.relevant_artifacts {
            out.push_str(&format!(
                "- `{}` ({}) — {} _(sources: {})_\n",
                artifact.relative_path.trim(),
                artifact.artifact_type.trim(),
                artifact.reason.trim(),
                format_sources(&artifact.source_event_ids)
            ));
        }
        out.push('\n');
    }

    out.push_str("## Limitations\n\n");
    if capsule.limitations.is_empty() {
        out.push_str("_None recorded._\n");
    } else {
        for limitation in &capsule.limitations {
            out.push_str(&format!("- {}\n", limitation.trim()));
        }
    }

    out
}

fn format_sources(ids: &[crate::EventId]) -> String {
    if ids.is_empty() {
        return "none".into();
    }
    ids.iter()
        .map(|id| format!("`{}`", id.0))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ApprovalService, Checkpoint, CheckpointId, Confidence, ErrorCode, EventId, EventPayload,
        FailedAttempt, Hypothesis, NextAction, NoteLabel, Project, ProjectId, RelevantArtifact,
        ResumeCapsule, Sensitivity, SourcedClaim, STATUS_DRAFT, STATUS_GENERATED, ThreadEvent,
        ThreadId, ThreadState, WorkThread,
    };
    use chrono::Utc;
    use serde_json::json;

    fn sample_capsule(event_id: &EventId, summary: &str) -> ResumeCapsule {
        ResumeCapsule {
            schema_version: "1.0".into(),
            goal: "Prevent duplicate provider capture on retry".into(),
            checkpoint_summary: summary.into(),
            verified_facts: vec![SourcedClaim {
                id: "fact-1".into(),
                claim: "Retry worker skips idempotency key lookup".into(),
                source_event_ids: vec![event_id.clone()],
            }],
            hypotheses: vec![Hypothesis {
                id: "hyp-1".into(),
                claim: "Adding a unique constraint on attempt_id prevents race".into(),
                confidence: Confidence::High,
                source_event_ids: vec![event_id.clone()],
            }],
            failed_attempts: vec![FailedAttempt {
                action: "In-memory mutex".into(),
                outcome: "Failed across multiple worker processes".into(),
                learning: "Lock must live in PostgreSQL".into(),
                source_event_ids: vec![event_id.clone()],
            }],
            unresolved_questions: vec!["Does webhook replay use the same key?".into()],
            relevant_artifacts: vec![RelevantArtifact {
                artifact_type: "file".into(),
                relative_path: "src/retry.rs".into(),
                reason: "Contains retry dispatch logic".into(),
                source_event_ids: vec![event_id.clone()],
            }],
            next_action: NextAction {
                instruction: "cargo test --test retry_lock".into(),
                justification: "Reproduces the race condition".into(),
                success_check: "Test passes with single provider call".into(),
                source_event_ids: vec![event_id.clone()],
            },
            limitations: vec!["Only tested against local stub".into()],
        }
    }

    #[test]
    fn unapproved_checkpoints_cannot_be_exported() {
        let db = Database::open_in_memory().unwrap();
        let now = Utc::now();
        let project = Project {
            id: ProjectId::new(),
            display_name: "Payments".into(),
            canonical_root: "/tmp/payments".into(),
            created_at: now,
            updated_at: now,
        };
        db.insert_project(&project).unwrap();
        let thread = WorkThread {
            id: ThreadId::new(),
            project_id: project.id,
            title: "Duplicate capture".into(),
            goal: "Prevent duplicate provider capture on retry".into(),
            state: ThreadState::CheckpointDraft,
            started_at: now,
            ended_at: None,
            created_at: now,
            updated_at: now,
        };
        db.insert_thread(&thread).unwrap();
        let ev = ThreadEvent::new(
            thread.id.clone(),
            EventPayload::NoteAdded {
                text: "evidence".into(),
                label: NoteLabel::Fact,
            },
            Sensitivity::Normal,
        )
        .unwrap();
        db.insert_event(&ev).unwrap();

        let draft_cp = Checkpoint {
            id: CheckpointId::new(),
            thread_id: thread.id.clone(),
            start_event_id: ev.id.clone(),
            end_event_id: ev.id.clone(),
            status: STATUS_GENERATED.into(),
            model_id: Some("gpt-5.4".into()),
            prompt_version: Some("v1".into()),
            payload: json!({}),
            draft: Some(sample_capsule(&ev.id, "UNAPPROVED DRAFT TEXT")),
            approved: None,
            created_at: now,
            approved_at: None,
        };
        db.insert_checkpoint(&draft_cp).unwrap();

        let exporter = ExportService::new(&db);
        assert_eq!(
            exporter.export_json(&draft_cp.id).unwrap_err().code,
            ErrorCode::InvalidState
        );
        assert_eq!(
            exporter.export_markdown(&draft_cp.id).unwrap_err().code,
            ErrorCode::InvalidState
        );
    }

    #[test]
    fn exports_contain_only_approved_capsule_and_are_deleted_with_thread() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/retry.rs"), "pub fn retry() {}\n").unwrap();

        let db = Database::open_in_memory().unwrap();
        let now = Utc::now();
        let project = Project {
            id: ProjectId::new(),
            display_name: "Payments".into(),
            canonical_root: std::fs::canonicalize(dir.path())
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            created_at: now,
            updated_at: now,
        };
        db.insert_project(&project).unwrap();
        let thread = WorkThread {
            id: ThreadId::new(),
            project_id: project.id,
            title: "Duplicate capture".into(),
            goal: "Prevent duplicate provider capture on retry".into(),
            state: ThreadState::CheckpointDraft,
            started_at: now,
            ended_at: None,
            created_at: now,
            updated_at: now,
        };
        db.insert_thread(&thread).unwrap();
        let ev = ThreadEvent::new(
            thread.id.clone(),
            EventPayload::NoteAdded {
                text: "evidence".into(),
                label: NoteLabel::Fact,
            },
            Sensitivity::Normal,
        )
        .unwrap();
        db.insert_event(&ev).unwrap();

        let draft_capsule = sample_capsule(&ev.id, "MODEL DRAFT SUMMARY DO NOT EXPORT");
        let approved_capsule = sample_capsule(&ev.id, "HUMAN APPROVED SUMMARY");

        let cp = Checkpoint {
            id: CheckpointId::new(),
            thread_id: thread.id.clone(),
            start_event_id: ev.id.clone(),
            end_event_id: ev.id.clone(),
            status: STATUS_DRAFT.into(),
            model_id: None,
            prompt_version: None,
            payload: json!({
                "version": 1,
                "threadTitle": thread.title,
                "goal": thread.goal,
                "trigger": "manual",
                "startEventId": ev.id,
                "endEventId": ev.id,
                "intervalEventCount": 1,
                "finalSnapshot": { "captured": false, "eventId": null, "reason": "none" },
                "entries": [{
                    "eventId": ev.id,
                    "timestamp": now,
                    "kind": "note.added",
                    "content": { "text": "evidence", "label": "fact" }
                }],
                "omissions": {
                    "duplicatesRemoved": 0,
                    "overBudgetRemoved": 0,
                    "notesShortened": 0,
                    "unreadable": 0
                },
                "totalChars": 10
            }),
            draft: None,
            approved: None,
            created_at: now,
            approved_at: None,
        };
        db.insert_checkpoint(&cp).unwrap();
        db.set_checkpoint_draft(&cp.id, &draft_capsule, "gpt-5.4", "v1")
            .unwrap();

        ApprovalService::new(&db)
            .approve(&cp.id, approved_capsule.clone())
            .unwrap();

        let exporter = ExportService::new(&db);
        let json_res = exporter.export_json(&cp.id).unwrap();
        let md_res = exporter.export_markdown(&cp.id).unwrap();

        let json_on_disk = std::fs::read_to_string(&json_res.path).unwrap();
        let md_on_disk = std::fs::read_to_string(&md_res.path).unwrap();

        assert!(!json_on_disk.contains("MODEL DRAFT SUMMARY DO NOT EXPORT"));
        assert!(!md_on_disk.contains("MODEL DRAFT SUMMARY DO NOT EXPORT"));
        assert!(json_on_disk.contains("HUMAN APPROVED SUMMARY"));
        assert!(md_on_disk.contains("HUMAN APPROVED SUMMARY"));
        assert!(md_on_disk.contains("cargo test --test retry_lock"));

        let parsed: ResumeCapsule = serde_json::from_str(&json_on_disk).unwrap();
        assert_eq!(parsed, approved_capsule);

        let receipt = db.delete_thread(&thread.id).unwrap();
        assert_eq!(receipt.exports_deleted, 2);
        assert!(!std::path::Path::new(&json_res.path).exists());
        assert!(!std::path::Path::new(&md_res.path).exists());
    }
}
