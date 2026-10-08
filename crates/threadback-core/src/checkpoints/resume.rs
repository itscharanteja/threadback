use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    Checkpoint, CheckpointId, CheckpointPayload, Correction, Database, DomainError, EventPayload,
    GitSnapshot, ProjectBoundary, STATUS_APPROVED, WorkThread, capture_snapshot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    BranchChanged,
    HeadChanged,
    WorkingTreeChanged,
    RepositoryUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DriftSignal {
    pub kind: DriftKind,
    pub label: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDrift {
    pub checked: bool,
    pub has_drift: bool,
    pub summary: String,
    pub signals: Vec<DriftSignal>,
    pub checkpoint_snapshot: Option<GitSnapshot>,
    pub current_snapshot: Option<GitSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResumeViewModel {
    pub thread: WorkThread,
    pub checkpoint: Checkpoint,
    pub corrections: Vec<Correction>,
    pub drift: RepositoryDrift,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenedArtifact {
    pub relative_path: String,
    pub resolved_path: String,
    pub exists: bool,
    pub content_preview: Option<String>,
    pub truncated: bool,
}

const MAX_ARTIFACT_PREVIEW_BYTES: usize = 32 * 1024;

/// Compares two repository snapshots and returns explicit drift signals.
pub fn compare_snapshots(before: &GitSnapshot, after: &GitSnapshot) -> Vec<DriftSignal> {
    let mut signals = Vec::new();

    if before.branch != after.branch {
        let before_label = before
            .branch
            .clone()
            .unwrap_or_else(|| "detached HEAD".into());
        let after_label = after
            .branch
            .clone()
            .unwrap_or_else(|| "detached HEAD".into());
        signals.push(DriftSignal {
            kind: DriftKind::BranchChanged,
            label: format!("Branch changed from {before_label} to {after_label}."),
            before: Some(before_label),
            after: Some(after_label),
        });
    }

    if before.head != after.head {
        let before_short = short_sha(before.head.as_deref());
        let after_short = short_sha(after.head.as_deref());
        signals.push(DriftSignal {
            kind: DriftKind::HeadChanged,
            label: format!("HEAD commit changed from {before_short} to {after_short}."),
            before: before.head.clone(),
            after: after.head.clone(),
        });
    }

    let before_paths = status_map(before);
    let after_paths = status_map(after);
    if before.dirty != after.dirty || before_paths != after_paths {
        let before_desc = describe_status(before);
        let after_desc = describe_status(after);
        signals.push(DriftSignal {
            kind: DriftKind::WorkingTreeChanged,
            label: format!("Working tree status changed ({before_desc} → {after_desc})."),
            before: Some(before_desc),
            after: Some(after_desc),
        });
    }

    signals
}

fn short_sha(sha: Option<&str>) -> String {
    match sha {
        Some(value) if value.len() >= 7 => value[..7].to_owned(),
        Some(value) => value.to_owned(),
        None => "no commits".into(),
    }
}

fn status_map(snapshot: &GitSnapshot) -> BTreeMap<String, (String, String)> {
    snapshot
        .changed_paths
        .iter()
        .map(|entry| {
            (
                entry.path.clone(),
                (entry.index_status.clone(), entry.worktree_status.clone()),
            )
        })
        .collect()
}

fn describe_status(snapshot: &GitSnapshot) -> String {
    if !snapshot.dirty {
        return "clean".into();
    }
    let count = snapshot.changed_paths.len();
    if count == 0 && snapshot.excluded_changes > 0 {
        return format!("{} excluded change(s)", snapshot.excluded_changes);
    }
    let names: Vec<&str> = snapshot
        .changed_paths
        .iter()
        .take(3)
        .map(|c| c.path.as_str())
        .collect();
    if count <= 3 {
        format!("{count} changed: {}", names.join(", "))
    } else {
        format!("{count} changed: {}, +{} more", names.join(", "), count - 3)
    }
}

/// Builds the return view model for an approved checkpoint, comparing the
/// checkpoint's recorded Git snapshot against the current repository state.
pub fn build_resume_view(
    database: &Database,
    checkpoint_id: &CheckpointId,
) -> Result<ResumeViewModel, DomainError> {
    let checkpoint = database
        .find_checkpoint(checkpoint_id)?
        .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
    let thread = database
        .find_thread(&checkpoint.thread_id)?
        .ok_or_else(|| DomainError::not_found("That thread no longer exists."))?;
    let corrections = database.list_corrections(checkpoint_id)?;
    let drift = compute_drift(database, &thread, &checkpoint)?;
    Ok(ResumeViewModel {
        thread,
        checkpoint,
        corrections,
        drift,
    })
}

fn compute_drift(
    database: &Database,
    thread: &WorkThread,
    checkpoint: &Checkpoint,
) -> Result<RepositoryDrift, DomainError> {
    let checkpoint_snapshot = extract_checkpoint_snapshot(database, &thread.id, checkpoint)?;
    let Some(project) = database.find_project(&thread.project_id)? else {
        return Ok(RepositoryDrift {
            checked: false,
            has_drift: true,
            summary: "The project is no longer registered.".into(),
            signals: vec![DriftSignal {
                kind: DriftKind::RepositoryUnavailable,
                label: "The project is no longer registered.".into(),
                before: None,
                after: None,
            }],
            checkpoint_snapshot,
            current_snapshot: None,
        });
    };

    let policy = database
        .find_project_policy(&project.id)
        .unwrap_or_else(|_| crate::ProjectPolicy::default_for(project.id.clone()));

    let boundary = match ProjectBoundary::new(&project.canonical_root, &policy.exclusions) {
        Ok(b) => b,
        Err(error) => {
            return Ok(RepositoryDrift {
                checked: false,
                has_drift: true,
                summary: error.message.clone(),
                signals: vec![DriftSignal {
                    kind: DriftKind::RepositoryUnavailable,
                    label: error.message,
                    before: None,
                    after: None,
                }],
                checkpoint_snapshot,
                current_snapshot: None,
            });
        }
    };

    let current_snapshot = match capture_snapshot(&boundary) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Ok(RepositoryDrift {
                checked: false,
                has_drift: true,
                summary: error.message.clone(),
                signals: vec![DriftSignal {
                    kind: DriftKind::RepositoryUnavailable,
                    label: error.message,
                    before: None,
                    after: None,
                }],
                checkpoint_snapshot,
                current_snapshot: None,
            });
        }
    };

    let Some(before) = checkpoint_snapshot else {
        return Ok(RepositoryDrift {
            checked: true,
            has_drift: false,
            summary: "No baseline Git snapshot was recorded at the checkpoint.".into(),
            signals: Vec::new(),
            checkpoint_snapshot: None,
            current_snapshot: Some(current_snapshot),
        });
    };

    let signals = compare_snapshots(&before, &current_snapshot);
    let has_drift = !signals.is_empty();
    let summary = if has_drift {
        format!(
            "Repository changed since this checkpoint ({} difference{}).",
            signals.len(),
            if signals.len() == 1 { "" } else { "s" }
        )
    } else {
        "Repository matches the checkpoint snapshot (same branch, HEAD, and working tree).".into()
    };

    Ok(RepositoryDrift {
        checked: true,
        has_drift,
        summary,
        signals,
        checkpoint_snapshot: Some(before),
        current_snapshot: Some(current_snapshot),
    })
}

fn extract_checkpoint_snapshot(
    database: &Database,
    thread_id: &crate::ThreadId,
    checkpoint: &Checkpoint,
) -> Result<Option<GitSnapshot>, DomainError> {
    if let Ok(payload) = serde_json::from_value::<CheckpointPayload>(checkpoint.payload.clone()) {
        for entry in payload.entries.iter().rev() {
            if entry.kind == "git.snapshot" {
                if let Ok(snapshot) = serde_json::from_value::<GitSnapshot>(entry.content.clone()) {
                    return Ok(Some(snapshot));
                }
            }
        }
    }

    let events = database.list_all_events(thread_id)?;
    for event in events.into_iter().rev() {
        if let EventPayload::GitSnapshot(value) = event.payload {
            if let Ok(snapshot) = serde_json::from_value::<GitSnapshot>(value) {
                return Ok(Some(snapshot));
            }
        }
    }
    Ok(None)
}

/// Validates and opens an artifact referenced by a checkpoint after an explicit
/// user click. Never opens excluded paths or paths outside the project root.
pub fn open_checkpoint_artifact(
    database: &Database,
    checkpoint_id: &CheckpointId,
    relative_path: &str,
) -> Result<OpenedArtifact, DomainError> {
    let checkpoint = database
        .find_checkpoint(checkpoint_id)?
        .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
    if checkpoint.status != STATUS_APPROVED {
        return Err(DomainError::invalid_state(
            "Artifacts can only be opened from an approved capsule.",
        ));
    }
    let thread = database
        .find_thread(&checkpoint.thread_id)?
        .ok_or_else(|| DomainError::not_found("That thread no longer exists."))?;
    let project = database
        .find_project(&thread.project_id)?
        .ok_or_else(|| DomainError::not_found("That project no longer exists."))?;
    let policy = database
        .find_project_policy(&project.id)
        .unwrap_or_else(|_| crate::ProjectPolicy::default_for(project.id.clone()));

    let boundary = match ProjectBoundary::new(&project.canonical_root, &policy.exclusions) {
        Ok(b) => b,
        Err(error) => {
            let in_approved = checkpoint
                .approved
                .as_ref()
                .map(|c| {
                    c.relevant_artifacts
                        .iter()
                        .any(|a| a.relative_path == relative_path)
                })
                .unwrap_or(false);
            if in_approved
                && !relative_path.is_empty()
                && !relative_path.starts_with('/')
                && !relative_path.split('/').any(|p| p == "..")
            {
                let fallback = std::path::Path::new(&project.canonical_root)
                    .join(relative_path)
                    .to_string_lossy()
                    .into_owned();
                return Ok(OpenedArtifact {
                    relative_path: relative_path.to_owned(),
                    resolved_path: fallback,
                    exists: false,
                    content_preview: None,
                    truncated: false,
                });
            }
            return Err(error);
        }
    };
    let resolved = boundary.resolve(relative_path)?;
    let full_path = boundary.root().join(&resolved.relative);
    let exists = full_path.is_file();

    let (content_preview, truncated) = if exists {
        match std::fs::read(&full_path) {
            Ok(bytes) => {
                let is_truncated = bytes.len() > MAX_ARTIFACT_PREVIEW_BYTES;
                let slice = &bytes[..bytes.len().min(MAX_ARTIFACT_PREVIEW_BYTES)];
                let text = String::from_utf8_lossy(slice).into_owned();
                let redacted = match crate::privacy::Redactor::new(&policy.redaction_patterns) {
                    Ok(redactor) => redactor.redact(&text).text,
                    Err(_) => crate::privacy::Redactor::builtin().redact(&text).text,
                };
                (Some(redacted), is_truncated)
            }
            Err(_) => (None, false),
        }
    } else {
        (None, false)
    };

    Ok(OpenedArtifact {
        relative_path: resolved.relative,
        resolved_path: full_path.to_string_lossy().into_owned(),
        exists,
        content_preview,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChangedPath, DiffStats};

    fn sample_snapshot(
        branch: Option<&str>,
        head: Option<&str>,
        dirty: bool,
        paths: &[&str],
    ) -> GitSnapshot {
        GitSnapshot {
            branch: branch.map(str::to_owned),
            head: head.map(str::to_owned),
            dirty,
            changed_paths: paths
                .iter()
                .map(|p| ChangedPath {
                    path: (*p).to_owned(),
                    index_status: " ".into(),
                    worktree_status: "M".into(),
                })
                .collect(),
            staged: DiffStats::default(),
            unstaged: DiffStats::default(),
            excluded_changes: 0,
            truncated: false,
        }
    }

    #[test]
    fn identical_snapshots_produce_no_drift_signals() {
        let a = sample_snapshot(Some("main"), Some("abcdef1234567"), true, &["src/lib.rs"]);
        let b = sample_snapshot(Some("main"), Some("abcdef1234567"), true, &["src/lib.rs"]);
        assert!(compare_snapshots(&a, &b).is_empty());
    }

    #[test]
    fn detects_branch_head_and_working_tree_drift() {
        let before = sample_snapshot(Some("main"), Some("1111111222222"), false, &[]);
        let after = sample_snapshot(
            Some("fix/retry"),
            Some("9999999888888"),
            true,
            &["src/retry.rs"],
        );
        let signals = compare_snapshots(&before, &after);
        assert_eq!(signals.len(), 3);
        assert_eq!(signals[0].kind, DriftKind::BranchChanged);
        assert!(signals[0].label.contains("main"));
        assert!(signals[0].label.contains("fix/retry"));
        assert_eq!(signals[1].kind, DriftKind::HeadChanged);
        assert!(signals[1].label.contains("1111111"));
        assert!(signals[1].label.contains("9999999"));
        assert_eq!(signals[2].kind, DriftKind::WorkingTreeChanged);
        assert!(signals[2].label.contains("src/retry.rs"));
    }

    #[test]
    fn formats_detached_head_and_more_than_three_changed_files_in_drift_signals() {
        let before = sample_snapshot(None, Some("aaaaaaa111111"), false, &[]);
        let after = sample_snapshot(
            Some("main"),
            Some("aaaaaaa111111"),
            true,
            &["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"],
        );
        let signals = compare_snapshots(&before, &after);
        assert_eq!(signals.len(), 2);
        assert_eq!(signals[0].kind, DriftKind::BranchChanged);
        assert_eq!(signals[0].before.as_deref(), Some("detached HEAD"));
        assert_eq!(signals[1].kind, DriftKind::WorkingTreeChanged);
        assert!(signals[1].after.as_deref().unwrap().contains("+2 more"));
    }
}
