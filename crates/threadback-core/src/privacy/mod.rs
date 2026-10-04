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
