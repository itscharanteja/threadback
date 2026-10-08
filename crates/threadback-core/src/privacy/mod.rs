pub mod redact;
pub mod stream;

use crate::{DomainError, EventPayload};

pub use redact::{
    Finding, FindingKind, MAX_CUSTOM_PATTERNS, MAX_PATTERN_CHARS, Redaction, Redactor, redact_value,
};
pub use stream::StreamRedactor;

/// Redacts every string in an event payload. Returns the new payload and the
/// number of credentials replaced, or `None` when nothing changed.
pub(crate) fn sanitize_payload(
    payload: &EventPayload,
) -> Result<Option<(EventPayload, u32)>, DomainError> {
    let mut value = serde_json::to_value(payload)
        .map_err(|error| DomainError::internal(format!("Payload serialization failed: {error}")))?;
    let findings = redact_value(Redactor::builtin(), &mut value);
    if findings.is_empty() {
        return Ok(None);
    }
    let secrets = findings
        .iter()
        .filter(|finding| finding.kind.is_secret())
        .map(|finding| finding.count)
        .sum();
    let payload = serde_json::from_value(value)
        .map_err(|error| DomainError::internal(format!("Payload rebuild failed: {error}")))?;
    Ok(Some((payload, secrets)))
}

/// Tests built-in + project/custom redaction rules against sample text without
/// storing anything.
pub fn test_redaction(
    database: &crate::Database,
    input: &crate::RedactionTestInput,
) -> Result<crate::RedactionTestResult, DomainError> {
    let mut patterns = input.custom_patterns.clone();
    if let Some(project_id) = &input.project_id {
        if let Ok(policy) = database.find_project_policy(project_id) {
            for pattern in policy.redaction_patterns {
                if !patterns.contains(&pattern) {
                    patterns.push(pattern);
                }
            }
        }
    }
    let redactor = Redactor::new(&patterns)?;
    let redaction = redactor.redact(&input.text);
    let secret_count = redaction.secret_count();
    let has_secret = redaction.has_secret();
    let findings = redaction
        .findings
        .into_iter()
        .map(|f| crate::RedactionTestFinding {
            label: f.kind.label().to_owned(),
            description: f.kind.description().to_owned(),
            count: f.count,
            is_secret: f.kind.is_secret(),
        })
        .collect();
    Ok(crate::RedactionTestResult {
        redacted_text: redaction.text,
        findings,
        secret_count,
        has_secret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Database, ErrorCode, NoteLabel, ProjectId, ProjectPolicy, RedactionTestInput};
    use chrono::Utc;

    #[test]
    fn test_redaction_merges_project_policy_patterns_and_never_persists_input() {
        let db = Database::open_in_memory().unwrap();
        let project_id = ProjectId("proj-sec-1".into());
        db.insert_project(&crate::Project {
            id: project_id.clone(),
            display_name: "SecOps".into(),
            canonical_root: "/tmp/secops".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .unwrap();

        let mut policy = ProjectPolicy::default_for(project_id.clone());
        policy.redaction_patterns = vec!["ACME_TOKEN_[A-Z0-9]{8}".into()];
        db.upsert_project_policy(&policy).unwrap();

        let res = test_redaction(
            &db,
            &RedactionTestInput {
                project_id: Some(project_id),
                custom_patterns: vec!["INTERNAL_KEY_[0-9]{4}".into()],
                text: "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig and ACME_TOKEN_1234ABCD and INTERNAL_KEY_9999".into(),
            },
        )
        .unwrap();

        assert!(res.has_secret);
        assert!(res.secret_count >= 3);
        assert!(!res.redacted_text.contains("ACME_TOKEN_1234ABCD"));
        assert!(!res.redacted_text.contains("INTERNAL_KEY_9999"));
        assert!(res.redacted_text.contains("[REDACTED:"));
    }

    #[test]
    fn test_redaction_rejects_invalid_custom_regex() {
        let db = Database::open_in_memory().unwrap();
        let err = test_redaction(
            &db,
            &RedactionTestInput {
                project_id: None,
                custom_patterns: vec!["[unclosed-group".into()],
                text: "hello".into(),
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn sanitize_payload_returns_none_for_clean_note_and_redacts_secret_note() {
        let clean = EventPayload::NoteAdded {
            text: "Normal observation with no credentials.".into(),
            label: NoteLabel::Fact,
        };
        assert!(sanitize_payload(&clean).unwrap().is_none());

        let secret = EventPayload::NoteAdded {
            text: "Token was sk_live_threadback_demo_99f18a4c2d8e4b60b5c7a1d2e3f40506".into(),
            label: NoteLabel::Fact,
        };
        let (sanitized, count) = sanitize_payload(&secret).unwrap().unwrap();
        assert!(count >= 1);
        if let EventPayload::NoteAdded { text, .. } = sanitized {
            assert!(!text.contains("sk_live_threadback_demo_99f18a4c2d8e4b60b5c7a1d2e3f40506"));
            assert!(text.contains("[REDACTED:"));
        } else {
            panic!("expected NoteAdded");
        }
    }
}
