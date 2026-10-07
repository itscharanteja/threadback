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
