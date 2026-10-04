use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::{
    Checkpoint, CheckpointId, CheckpointPayload, CheckpointTrigger, Database, DomainError, EventId,
    Finding, Omissions, PreviewDisposition, Redactor, Sensitivity, SnapshotStatus,
    privacy::redact_value,
};

pub const OUTBOUND_SCHEMA_VERSION: u32 = 1;

/// Entry kinds that may ever be sent. Anything else stays on this computer,
/// so a new kind of evidence is local until someone decides otherwise.
const SENDABLE_KINDS: [&str; 8] = [
    "thread.started",
    "thread.paused",
    "thread.resumed",
    "note.added",
    "git.snapshot",
    "command.started",
    "command.finished",
    "file.changed",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFinding {
    /// What was found, in a sentence fragment such as "a bearer token".
    pub description: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PayloadPreviewEntry {
    pub event_id: EventId,
    pub timestamp: DateTime<Utc>,
    pub kind: String,
    pub disposition: PreviewDisposition,
    /// Exactly what would be sent for an included entry. For a blocked entry
    /// it is the text with the offending parts replaced, so a credential is
    /// never shown again.
    pub value: Value,
    /// Size of `value` as sent.
    pub size_bytes: u32,
    pub reason: Option<String>,
    pub findings: Vec<PreviewFinding>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCounts {
    pub included: u32,
    pub excluded: u32,
    pub blocked: u32,
    pub local_only: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PayloadPreview {
    pub checkpoint_id: CheckpointId,
    pub thread_title: String,
    pub goal: String,
    pub trigger: CheckpointTrigger,
    pub final_snapshot: SnapshotStatus,
    pub entries: Vec<PayloadPreviewEntry>,
    pub counts: PreviewCounts,
    /// Changes made to the title, goal, or snapshot note, which cannot be
    /// excluded.
    pub notices: Vec<String>,
    /// Size of everything that would be sent.
    pub total_size_bytes: u32,
    /// The exact JSON document that would be sent, as serialized.
    pub payload_json: String,
    /// Identifies exactly what would be sent. Approval must present it back.
    pub payload_hash: String,
    pub has_blocking_findings: bool,
    /// False while anything is blocked or nothing is selected.
    pub can_generate: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OutboundEntry {
    pub event_id: EventId,
    pub timestamp: DateTime<Utc>,
    pub kind: String,
    pub content: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OutboundInterval {
    pub start_event_id: EventId,
    pub end_event_id: EventId,
    pub event_count: u32,
}

/// The evidence a model request is built from. What the user reviews is this
/// document, serialized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OutboundPayload {
    pub schema_version: u32,
    pub thread_title: String,
    pub goal: String,
    pub trigger: CheckpointTrigger,
    pub interval: OutboundInterval,
    pub final_snapshot: SnapshotStatus,
    pub entries: Vec<OutboundEntry>,
    pub omissions: Omissions,
}

/// A payload that passed every check and matches what the user reviewed. It
/// can only be made by `OutboundService::authorize`, so code that sends to a
/// model can demand one and have no other way to get content.
#[derive(Debug, Clone)]
pub struct ApprovedPayload {
    checkpoint_id: CheckpointId,
    document: OutboundPayload,
    json: String,
    hash: String,
}

impl ApprovedPayload {
    /// For in-crate tests that need a payload without a database. It is not
    /// compiled into the library, so it cannot be used to skip `authorize`.
    #[cfg(test)]
    pub(crate) fn for_tests(document: OutboundPayload) -> Self {
        let json = serde_json::to_string(&document).expect("document serializes");
        let hash = Sha256::digest(json.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self {
            checkpoint_id: CheckpointId::new(),
            document,
            json,
            hash,
        }
    }

    pub fn checkpoint_id(&self) -> &CheckpointId {
        &self.checkpoint_id
    }

    pub fn document(&self) -> &OutboundPayload {
        &self.document
    }

    /// The exact bytes of the document, as reviewed.
    pub fn json(&self) -> &str {
        &self.json
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn size_bytes(&self) -> usize {
        self.json.len()
    }
}

pub struct OutboundService<'a> {
    database: &'a Database,
    redactor: &'a Redactor,
}

impl<'a> OutboundService<'a> {
    /// Uses the built-in rules.
    pub fn new(database: &'a Database) -> Self {
        Self {
            database,
            redactor: Redactor::builtin(),
        }
    }

    /// Adds a project's custom patterns to the scan made before sending.
    pub fn with_redactor(database: &'a Database, redactor: &'a Redactor) -> Self {
        Self { database, redactor }
    }

    /// What would be sent for this checkpoint, entry by entry, with the user's
    /// exclusions applied.
    pub fn prepare(
        &self,
        checkpoint_id: &CheckpointId,
        excluded: &[EventId],
    ) -> Result<PayloadPreview, DomainError> {
        Ok(self.assemble(checkpoint_id, excluded)?.0)
    }

    /// The only way to obtain an `ApprovedPayload`. It refuses while anything
    /// is blocked, when nothing is selected, and when what would be sent is
    /// not what the user reviewed (`reviewed_hash` is the preview's
    /// `payloadHash`).
    pub fn authorize(
        &self,
        checkpoint_id: &CheckpointId,
        excluded: &[EventId],
        reviewed_hash: &str,
    ) -> Result<ApprovedPayload, DomainError> {
        let (preview, document, json) = self.assemble(checkpoint_id, excluded)?;
        if preview.has_blocking_findings {
            return Err(DomainError::payload_blocked(
                "Something in this payload contains a credential. Exclude those entries before generating.",
            ));
        }
        if preview.counts.included == 0 {
            return Err(DomainError::payload_blocked(
                "Nothing is selected to send. Include at least one entry.",
            ));
        }
        if preview.payload_hash != reviewed_hash {
            return Err(DomainError::payload_changed(
                "What would be sent has changed since you reviewed it. Review it again.",
            ));
        }
        Ok(ApprovedPayload {
            checkpoint_id: checkpoint_id.clone(),
            document,
            json,
            hash: preview.payload_hash,
        })
    }

    /// The context a reply is judged in: the entries the user approved are the
    /// only ones it may cite, and the other entries of the same checkpoint are
    /// known to have been withheld.
    pub fn validation_context(
        &self,
        approved: &ApprovedPayload,
        boundary: crate::ProjectBoundary,
    ) -> Result<crate::ValidationContext, DomainError> {
        let checkpoint = self
            .database
            .find_checkpoint(approved.checkpoint_id())?
            .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
        let payload: CheckpointPayload = serde_json::from_value(checkpoint.payload)
            .map_err(|_| DomainError::internal("The checkpoint draft could not be read."))?;
        let sent: HashSet<&EventId> = approved
            .document()
            .entries
            .iter()
            .map(|e| &e.event_id)
            .collect();
        let withheld = payload
            .entries
            .iter()
            .filter(|entry| !sent.contains(&entry.event_id))
            .map(|entry| entry.event_id.clone());
        Ok(crate::ValidationContext::from_approved(
            approved, withheld, boundary,
        ))
    }

    /// Asks for a capsule to be generated from the reviewed payload. The
    /// reviewed payload is authorized first, so a blocked, empty, or changed
    /// payload is refused here whatever the caller does.
    ///
    /// The model connection is not built yet, so a payload that passes every
    /// check ends in `NOT_CONFIGURED` and nothing is sent.
    pub fn generate(
        &self,
        checkpoint_id: &CheckpointId,
        excluded: &[EventId],
        reviewed_hash: &str,
    ) -> Result<(), DomainError> {
        let _approved = self.authorize(checkpoint_id, excluded, reviewed_hash)?;
        Err(DomainError::not_configured(
            "The model connection is not set up yet. Nothing was sent.",
        ))
    }

    fn assemble(
        &self,
        checkpoint_id: &CheckpointId,
        excluded: &[EventId],
    ) -> Result<(PayloadPreview, OutboundPayload, String), DomainError> {
        let checkpoint = self
            .database
            .find_checkpoint(checkpoint_id)?
            .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
        if checkpoint.status == crate::STATUS_DISCARDED {
            return Err(DomainError::invalid_state(
                "That draft was discarded, so there is nothing to send.",
            ));
        }
        let payload: CheckpointPayload = serde_json::from_value(checkpoint.payload.clone())
            .map_err(|_| DomainError::internal("The checkpoint draft could not be read."))?;

        let known: HashSet<&EventId> = payload
            .entries
            .iter()
            .map(|entry| &entry.event_id)
            .collect();
        if let Some(unknown) = excluded.iter().find(|id| !known.contains(id)) {
            return Err(DomainError::invalid_input(
                format!("{} is not part of this checkpoint.", unknown.0),
                "excludedEventIds",
            ));
        }
        let sensitivities: HashMap<EventId, Sensitivity> = self
            .database
            .list_all_events(&checkpoint.thread_id)?
            .into_iter()
            .map(|event| (event.id, event.sensitivity))
            .collect();

        Ok(build(
            self.redactor,
            &checkpoint,
            &payload,
            &sensitivities,
            &excluded.iter().cloned().collect(),
        ))
    }
}

fn serialized_size(value: &Value) -> u32 {
    serde_json::to_string(value).map_or(0, |text| text.len().min(u32::MAX as usize) as u32)
}

fn describe(findings: &[Finding]) -> String {
    let parts: Vec<String> = findings
        .iter()
        .map(|finding| {
            if finding.count > 1 {
                format!("{} ({}×)", finding.kind.description(), finding.count)
            } else {
                finding.kind.description().to_owned()
            }
        })
        .collect();
    parts.join(", ")
}

fn preview_findings(findings: &[Finding]) -> Vec<PreviewFinding> {
    findings
        .iter()
        .map(|finding| PreviewFinding {
            description: finding.kind.description().to_owned(),
            count: finding.count,
        })
        .collect()
}

/// Header text (title, goal, snapshot note) cannot be excluded, so a match is
/// replaced rather than blocked, and the user is told.
fn clean_header(redactor: &Redactor, label: &str, text: &str, notices: &mut Vec<String>) -> String {
    let result = redactor.redact(text);
    if result.findings.is_empty() {
        return result.text;
    }
    notices.push(format!(
        "The {label} contained {}, which was replaced before sending.",
        describe(&result.findings)
    ));
    result.text
}

fn build(
    redactor: &Redactor,
    checkpoint: &Checkpoint,
    payload: &CheckpointPayload,
    sensitivities: &HashMap<EventId, Sensitivity>,
    excluded: &HashSet<EventId>,
) -> (PayloadPreview, OutboundPayload, String) {
    let mut notices = Vec::new();
    let thread_title = clean_header(redactor, "title", &payload.thread_title, &mut notices);
    let goal = clean_header(redactor, "goal", &payload.goal, &mut notices);
    let mut final_snapshot = payload.final_snapshot.clone();
    if let Some(reason) = &final_snapshot.reason {
        final_snapshot.reason = Some(clean_header(
            redactor,
            "snapshot note",
            reason,
            &mut notices,
        ));
    }

    let mut entries = Vec::new();
    let mut outbound = Vec::new();
    let mut counts = PreviewCounts::default();

    for entry in &payload.entries {
        let mut value = entry.content.clone();
        let findings = redact_value(redactor, &mut value);
        let secrets: Vec<Finding> = findings
            .iter()
            .filter(|finding| finding.kind.is_secret())
            .cloned()
            .collect();

        let (disposition, reason) = if !SENDABLE_KINDS.contains(&entry.kind.as_str()) {
            (
                PreviewDisposition::LocalOnly,
                Some("This kind of evidence is never sent.".to_owned()),
            )
        } else if sensitivities.get(&entry.event_id) == Some(&Sensitivity::Blocked) {
            (
                PreviewDisposition::LocalOnly,
                Some("This evidence was marked as never to be sent.".to_owned()),
            )
        } else if excluded.contains(&entry.event_id) {
            (
                PreviewDisposition::Excluded,
                Some("Excluded by you.".to_owned()),
            )
        } else if !secrets.is_empty() {
            (
                PreviewDisposition::Blocked,
                Some(format!(
                    "Contains {} that would be sent. Exclude this entry to continue.",
                    describe(&secrets)
                )),
            )
        } else if !findings.is_empty() {
            (
                PreviewDisposition::Included,
                Some("Home-directory paths were rewritten to ~.".to_owned()),
            )
        } else {
            (PreviewDisposition::Included, None)
        };

        match disposition {
            PreviewDisposition::Included => {
                counts.included += 1;
                outbound.push(OutboundEntry {
                    event_id: entry.event_id.clone(),
                    timestamp: entry.timestamp,
                    kind: entry.kind.clone(),
                    content: value.clone(),
                });
            }
            PreviewDisposition::Excluded => counts.excluded += 1,
            PreviewDisposition::Blocked => counts.blocked += 1,
            PreviewDisposition::LocalOnly => counts.local_only += 1,
        }
        entries.push(PayloadPreviewEntry {
            event_id: entry.event_id.clone(),
            timestamp: entry.timestamp,
            kind: entry.kind.clone(),
            disposition,
            size_bytes: serialized_size(&value),
            value,
            reason,
            findings: preview_findings(&findings),
        });
    }

    let document = OutboundPayload {
        schema_version: OUTBOUND_SCHEMA_VERSION,
        thread_title: thread_title.clone(),
        goal: goal.clone(),
        trigger: payload.trigger,
        interval: OutboundInterval {
            start_event_id: payload.start_event_id.clone(),
            end_event_id: payload.end_event_id.clone(),
            event_count: payload.interval_event_count,
        },
        final_snapshot: final_snapshot.clone(),
        entries: outbound,
        omissions: payload.omissions.clone(),
    };
    let json = serde_json::to_string(&document).unwrap_or_default();
    let payload_hash = Sha256::digest(json.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let has_blocking_findings = counts.blocked > 0;
    let preview = PayloadPreview {
        checkpoint_id: checkpoint.id.clone(),
        thread_title,
        goal,
        trigger: payload.trigger,
        final_snapshot,
        can_generate: !has_blocking_findings && counts.included > 0,
        has_blocking_findings,
        total_size_bytes: json.len().min(u32::MAX as usize) as u32,
        payload_json: json.clone(),
        payload_hash,
        counts,
        notices,
        entries,
    };
    (preview, document, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ThreadId, privacy::FindingKind};
    use serde_json::json;

    fn entry(id: &str, kind: &str, content: Value) -> crate::CheckpointEntry {
        crate::CheckpointEntry {
            event_id: EventId(id.into()),
            timestamp: DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap(),
            kind: kind.into(),
            content,
        }
    }

    fn checkpoint(entries: Vec<crate::CheckpointEntry>) -> (Checkpoint, CheckpointPayload) {
        let payload = CheckpointPayload {
            version: 1,
            thread_title: "Duplicate capture".into(),
            goal: "Find the second provider call".into(),
            trigger: CheckpointTrigger::Manual,
            start_event_id: EventId("start".into()),
            end_event_id: EventId("end".into()),
            interval_event_count: entries.len() as u32,
            final_snapshot: SnapshotStatus {
                captured: true,
                event_id: None,
                reason: None,
            },
            entries,
            omissions: Omissions::default(),
            total_chars: 0,
        };
        let checkpoint = Checkpoint {
            id: CheckpointId::new(),
            thread_id: ThreadId::new(),
            start_event_id: EventId("start".into()),
            end_event_id: EventId("end".into()),
            status: "draft".into(),
            model_id: None,
            prompt_version: None,
            payload: serde_json::to_value(&payload).unwrap(),
            draft: None,
            approved: None,
            created_at: Utc::now(),
            approved_at: None,
        };
        (checkpoint, payload)
    }

    fn run(
        redactor: &Redactor,
        entries: Vec<crate::CheckpointEntry>,
        sensitivities: &[(&str, Sensitivity)],
        excluded: &[&str],
    ) -> (PayloadPreview, OutboundPayload, String) {
        let (checkpoint, payload) = checkpoint(entries);
        let sensitivities = sensitivities
            .iter()
            .map(|(id, sensitivity)| (EventId((*id).into()), sensitivity.clone()))
            .collect();
        let excluded = excluded.iter().map(|id| EventId((*id).into())).collect();
        build(redactor, &checkpoint, &payload, &sensitivities, &excluded)
    }

    fn custom() -> Redactor {
        Redactor::new(&["ACME-[0-9]{6}".into()]).unwrap()
    }

    fn disposition(preview: &PayloadPreview, id: &str) -> PreviewDisposition {
        preview
            .entries
            .iter()
            .find(|e| e.event_id.0 == id)
            .unwrap()
            .disposition
    }

    #[test]
    fn a_clean_checkpoint_is_included_whole_with_exact_sizes() {
        let (preview, document, json) = run(
            Redactor::builtin(),
            vec![
                entry(
                    "a",
                    "note.added",
                    json!({"label": "fact", "text": "two provider calls"}),
                ),
                entry("b", "command.finished", json!({"exitCode": 1})),
            ],
            &[],
            &[],
        );
        assert_eq!(
            preview.counts,
            PreviewCounts {
                included: 2,
                excluded: 0,
                blocked: 0,
                local_only: 0
            }
        );
        assert!(preview.can_generate && !preview.has_blocking_findings);
        for entry in &preview.entries {
            assert_eq!(
                entry.size_bytes as usize,
                serde_json::to_string(&entry.value).unwrap().len()
            );
            assert!(entry.reason.is_none() && entry.findings.is_empty());
        }
        assert_eq!(preview.total_size_bytes as usize, json.len());
        assert_eq!(document.entries.len(), 2);
        assert_eq!(preview.payload_hash.len(), 64);
    }

    #[test]
    fn the_hash_is_stable_and_changes_with_what_is_sent() {
        let entries = || {
            vec![
                entry("a", "note.added", json!({"text": "one"})),
                entry("b", "note.added", json!({"text": "two"})),
            ]
        };
        let hash = |excluded: &[&str]| {
            run(Redactor::builtin(), entries(), &[], excluded)
                .0
                .payload_hash
        };
        assert_eq!(hash(&[]), hash(&[]));
        assert_ne!(hash(&[]), hash(&["a"]));
        assert_ne!(hash(&["a"]), hash(&["b"]));
    }

    #[test]
    fn a_match_is_blocked_and_never_shown_again() {
        let (preview, document, json) = run(
            &custom(),
            vec![
                entry("ok", "note.added", json!({"text": "fine"})),
                entry(
                    "bad",
                    "note.added",
                    json!({"text": "culprit is ACME-123456 and ACME-654321"}),
                ),
            ],
            &[],
            &[],
        );
        assert_eq!(disposition(&preview, "bad"), PreviewDisposition::Blocked);
        let bad = preview
            .entries
            .iter()
            .find(|e| e.event_id.0 == "bad")
            .unwrap();
        assert_eq!(
            bad.value["text"],
            "culprit is [REDACTED:custom] and [REDACTED:custom]"
        );
        assert!(
            bad.reason
                .as_deref()
                .unwrap()
                .contains("text matching one of your custom patterns (2×)")
        );
        assert_eq!(
            bad.findings,
            [PreviewFinding {
                description: "text matching one of your custom patterns".into(),
                count: 2
            }]
        );
        assert!(preview.has_blocking_findings && !preview.can_generate);
        assert_eq!(preview.counts.blocked, 1);
        // A blocked entry is not part of the document.
        assert!(!json.contains("ACME-") && document.entries.len() == 1);
        assert!(
            !serde_json::to_string(&preview)
                .unwrap()
                .contains("ACME-123456")
        );
    }

    #[test]
    fn excluding_a_blocked_entry_clears_the_block() {
        let entries = || {
            vec![
                entry("ok", "note.added", json!({"text": "fine"})),
                entry("bad", "note.added", json!({"text": "ACME-123456"})),
            ]
        };
        let (preview, _, _) = run(&custom(), entries(), &[], &["bad"]);
        assert_eq!(disposition(&preview, "bad"), PreviewDisposition::Excluded);
        assert!(!preview.has_blocking_findings && preview.can_generate);
        assert_eq!(
            preview.counts,
            PreviewCounts {
                included: 1,
                excluded: 1,
                blocked: 0,
                local_only: 0
            }
        );
    }

    #[test]
    fn unknown_kinds_and_blocked_sensitivity_stay_local() {
        let (preview, document, _) = run(
            Redactor::builtin(),
            vec![
                entry("keys", "terminal.keystrokes", json!({"text": "hello"})),
                entry("never", "note.added", json!({"text": "private"})),
                entry("ok", "note.added", json!({"text": "fine"})),
            ],
            &[("never", Sensitivity::Blocked)],
            &[],
        );
        assert_eq!(disposition(&preview, "keys"), PreviewDisposition::LocalOnly);
        assert_eq!(
            disposition(&preview, "never"),
            PreviewDisposition::LocalOnly
        );
        assert_eq!(preview.counts.local_only, 2);
        assert_eq!(document.entries.len(), 1);
        assert!(
            preview.can_generate,
            "local-only entries do not block anything"
        );
    }

    #[test]
    fn local_only_wins_over_everything_and_exclusion_wins_over_a_block() {
        let (preview, _, _) = run(
            &custom(),
            vec![
                entry(
                    "keys",
                    "terminal.keystrokes",
                    json!({"text": "ACME-123456"}),
                ),
                entry("bad", "note.added", json!({"text": "ACME-123456"})),
            ],
            &[],
            &["keys", "bad"],
        );
        assert_eq!(disposition(&preview, "keys"), PreviewDisposition::LocalOnly);
        assert_eq!(disposition(&preview, "bad"), PreviewDisposition::Excluded);
    }

    #[test]
    fn home_paths_are_rewritten_not_blocked() {
        let (preview, document, json) = run(
            Redactor::builtin(),
            vec![entry(
                "p",
                "command.finished",
                json!({"stdout": {"text": "error in /Users/someone/app/a.rs"}}),
            )],
            &[],
            &[],
        );
        assert_eq!(disposition(&preview, "p"), PreviewDisposition::Included);
        assert_eq!(
            document.entries[0].content["stdout"]["text"],
            "error in ~/app/a.rs"
        );
        assert!(
            preview.entries[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("rewritten")
        );
        assert!(!json.contains("someone"));
        assert!(preview.can_generate);
    }

    #[test]
    fn a_match_in_the_goal_or_title_is_replaced_and_reported() {
        let (mut checkpoint, mut payload) =
            checkpoint(vec![entry("a", "note.added", json!({"text": "ok"}))]);
        payload.goal = "Why does ACME-123456 fail?".into();
        payload.thread_title = "Investigate ACME-777777".into();
        payload.final_snapshot.reason = Some("Git failed at /Users/someone/repo".into());
        checkpoint.payload = serde_json::to_value(&payload).unwrap();
        let (preview, document, json) = build(
            &custom(),
            &checkpoint,
            &payload,
            &HashMap::new(),
            &HashSet::new(),
        );
        assert_eq!(preview.goal, "Why does [REDACTED:custom] fail?");
        assert_eq!(document.thread_title, "Investigate [REDACTED:custom]");
        assert_eq!(
            document.final_snapshot.reason.as_deref(),
            Some("Git failed at ~/repo")
        );
        assert_eq!(preview.notices.len(), 3);
        assert!(preview.notices[0].contains("title") && preview.notices[1].contains("goal"));
        assert!(preview.notices[2].contains("snapshot note"));
        assert!(
            preview.can_generate,
            "the header cannot be excluded, so it is replaced, not blocked"
        );
        assert!(!json.contains("ACME-") && !json.contains("someone"));
    }

    #[test]
    fn nothing_selected_cannot_be_generated() {
        let (preview, _, _) = run(
            Redactor::builtin(),
            vec![entry("a", "note.added", json!({"text": "x"}))],
            &[],
            &["a"],
        );
        assert!(!preview.can_generate && !preview.has_blocking_findings);
        assert_eq!(preview.counts.included, 0);
    }

    #[test]
    fn every_finding_kind_has_a_sentence() {
        for kind in [
            FindingKind::PrivateKey,
            FindingKind::BearerToken,
            FindingKind::UrlPassword,
            FindingKind::UrlToken,
            FindingKind::ProviderToken,
            FindingKind::Jwt,
            FindingKind::SecretAssignment,
            FindingKind::Custom,
            FindingKind::HomePath,
        ] {
            assert!(!kind.description().is_empty());
        }
    }
}
