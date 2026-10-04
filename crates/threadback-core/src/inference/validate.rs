use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

use super::schema::CapsuleSchema;
use crate::{ApprovedPayload, EventId, NoteLabel, ProjectBoundary, Redactor, ResumeCapsule};

/// What kind of problem a reply has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViolationKind {
    /// The reply is not one JSON object.
    NotJson,
    /// The reply does not match the capsule schema.
    Shape,
    /// A cited event is not part of the evidence the user approved.
    Provenance,
    /// A verified fact has nothing that can support it.
    UnsupportedFact,
    DuplicateId,
    ArtifactPath,
    /// Text that looks like a credential.
    Credential,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    /// Where in the reply, such as `/verified_facts/0/source_event_ids/1`.
    pub path: String,
    pub kind: ViolationKind,
    pub message: String,
}

/// An entry the model was shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownEntry {
    pub event_id: EventId,
    pub kind: String,
    /// The label the user chose, for a note.
    pub note_label: Option<NoteLabel>,
}

impl ShownEntry {
    /// An entry as it appears in a checkpoint or an outbound payload: its
    /// `content` carries the label the user chose when it is a note.
    pub fn from_content(event_id: EventId, kind: &str, content: &Value) -> Self {
        Self {
            note_label: (kind == "note.added")
                .then(|| serde_json::from_value(content["label"].clone()).ok())
                .flatten(),
            kind: kind.to_owned(),
            event_id,
        }
    }
}

/// Everything needed to judge a capsule against the evidence that produced it.
pub struct ValidationContext {
    shown: HashMap<EventId, ShownEntry>,
    /// In the checkpoint, but excluded, blocked, or kept local, so never shown.
    withheld: HashSet<EventId>,
    boundary: ProjectBoundary,
    redactor: &'static Redactor,
}

impl ValidationContext {
    pub fn new(
        shown: impl IntoIterator<Item = ShownEntry>,
        withheld: impl IntoIterator<Item = EventId>,
        boundary: ProjectBoundary,
    ) -> Self {
        Self {
            shown: shown
                .into_iter()
                .map(|entry| (entry.event_id.clone(), entry))
                .collect(),
            withheld: withheld.into_iter().collect(),
            boundary,
            redactor: Redactor::builtin(),
        }
    }

    /// The entries of the payload the user approved are the only ones a
    /// capsule may cite.
    pub fn from_approved(
        approved: &ApprovedPayload,
        withheld: impl IntoIterator<Item = EventId>,
        boundary: ProjectBoundary,
    ) -> Self {
        let shown = approved.document().entries.iter().map(|entry| {
            ShownEntry::from_content(entry.event_id.clone(), &entry.kind, &entry.content)
        });
        Self::new(shown, withheld, boundary)
    }
}

/// The result of judging one reply.
#[derive(Debug, Clone, PartialEq)]
pub enum Evaluation {
    Valid(Box<ResumeCapsule>),
    Invalid(Vec<Violation>),
}

/// The most problems reported for one reply, so a correction request stays short.
pub const MAX_VIOLATIONS: usize = 20;

/// Parses a reply and judges it: one JSON object, the schema, then the
/// evidence. A reply wrapped in prose or a code fence is not accepted as it
/// stands; it goes to the correction step like any other defect.
pub fn evaluate(reply: &str, context: &ValidationContext) -> Evaluation {
    let value: Value = match serde_json::from_str(reply.trim()) {
        Ok(value) => value,
        Err(_) => {
            let hint = if reply.trim_start().starts_with("```") {
                "The reply is wrapped in a code fence. Reply with the bare JSON object only."
            } else if !reply.trim_start().starts_with('{') {
                "The reply is not a JSON object. Reply with one JSON object and no other text."
            } else {
                "The reply is not valid JSON, or has text around the JSON object. Reply with one JSON object only."
            };
            return Evaluation::Invalid(vec![Violation {
                path: String::new(),
                kind: ViolationKind::NotJson,
                message: hint.into(),
            }]);
        }
    };

    let schema = match CapsuleSchema::new() {
        Ok(schema) => schema,
        Err(_) => {
            return Evaluation::Invalid(vec![Violation {
                path: String::new(),
                kind: ViolationKind::Shape,
                message: "The capsule schema could not be loaded.".into(),
            }]);
        }
    };
    let issues = schema.check(&value);
    if !issues.is_empty() {
        let mut violations: Vec<Violation> = issues
            .into_iter()
            .map(|issue| Violation {
                path: issue.path,
                kind: ViolationKind::Shape,
                message: issue.message,
            })
            .collect();
        violations.truncate(MAX_VIOLATIONS);
        return Evaluation::Invalid(violations);
    }

    let capsule: ResumeCapsule = match serde_json::from_value(value) {
        Ok(capsule) => capsule,
        Err(error) => {
            return Evaluation::Invalid(vec![Violation {
                path: String::new(),
                kind: ViolationKind::Shape,
                message: format!("The reply does not match the capsule contract: {error}"),
            }]);
        }
    };
    let mut violations = validate_capsule(&capsule, context);
    if violations.is_empty() {
        Evaluation::Valid(Box::new(capsule))
    } else {
        violations.truncate(MAX_VIOLATIONS);
        Evaluation::Invalid(violations)
    }
}

/// Judges a capsule a person has edited by the same two checks the model's
/// reply meets: the schema (limits on length and size), then the evidence.
/// Returns every problem, in a stable order, and none when it is acceptable.
pub fn check_capsule(capsule: &ResumeCapsule, context: &ValidationContext) -> Vec<Violation> {
    let value = match serde_json::to_value(capsule) {
        Ok(value) => value,
        Err(_) => {
            return vec![Violation {
                path: String::new(),
                kind: ViolationKind::Shape,
                message: "The capsule could not be read.".into(),
            }];
        }
    };
    let schema = match CapsuleSchema::new() {
        Ok(schema) => schema,
        Err(_) => {
            return vec![Violation {
                path: String::new(),
                kind: ViolationKind::Shape,
                message: "The capsule schema could not be loaded.".into(),
            }];
        }
    };
    let issues = schema.check(&value);
    if !issues.is_empty() {
        let mut violations: Vec<Violation> = issues
            .into_iter()
            .map(|issue| Violation {
                path: issue.path,
                kind: ViolationKind::Shape,
                message: issue.message,
            })
            .collect();
        violations.sort();
        violations.dedup();
        return violations;
    }
    validate_capsule(capsule, context)
}

fn is_uncertain_note(entry: &ShownEntry) -> bool {
    matches!(
        entry.note_label,
        Some(NoteLabel::Hypothesis | NoteLabel::Question | NoteLabel::NextStep)
    )
}

/// Checks a well-formed capsule against the evidence. It is used on the
/// model's reply and again when a person edits a capsule, so an edit cannot
/// create an unsupported verified fact either. Returns every problem, in a
/// stable order.
pub fn validate_capsule(capsule: &ResumeCapsule, context: &ValidationContext) -> Vec<Violation> {
    let mut violations = Vec::new();

    let mut check_sources = |section: &str, index: usize, ids: &[EventId]| {
        for (position, id) in ids.iter().enumerate() {
            if context.shown.contains_key(id) {
                continue;
            }
            let why = if context.withheld.contains(id) {
                "is in this checkpoint but was not part of the evidence you approved"
            } else {
                "is not an event in this checkpoint"
            };
            violations.push(Violation {
                path: format!("/{section}/{index}/source_event_ids/{position}"),
                kind: ViolationKind::Provenance,
                message: format!(
                    "The cited event {} {why}. Cite only ids from the eventId fields.",
                    clip(&id.0, 60)
                ),
            });
        }
    };
    for (index, fact) in capsule.verified_facts.iter().enumerate() {
        check_sources("verified_facts", index, &fact.source_event_ids);
    }
    for (index, hypothesis) in capsule.hypotheses.iter().enumerate() {
        check_sources("hypotheses", index, &hypothesis.source_event_ids);
    }
    for (index, attempt) in capsule.failed_attempts.iter().enumerate() {
        check_sources("failed_attempts", index, &attempt.source_event_ids);
    }
    for (index, artifact) in capsule.relevant_artifacts.iter().enumerate() {
        check_sources("relevant_artifacts", index, &artifact.source_event_ids);
    }
    check_sources("next_action", 0, &capsule.next_action.source_event_ids);
    // `next_action` is an object, not a list, so its path has no index.
    for violation in &mut violations {
        if let Some(rest) = violation.path.strip_prefix("/next_action/0/") {
            violation.path = format!("/next_action/{rest}");
        }
    }

    // A verified fact needs at least one source that can support a fact. A
    // note the user labelled hypothesis, question, or next step cannot.
    for (index, fact) in capsule.verified_facts.iter().enumerate() {
        let known: Vec<&ShownEntry> = fact
            .source_event_ids
            .iter()
            .filter_map(|id| context.shown.get(id))
            .collect();
        let has_support = known.iter().any(|entry| !is_uncertain_note(entry));
        if !known.is_empty() && !has_support {
            violations.push(Violation {
                path: format!("/verified_facts/{index}/source_event_ids"),
                kind: ViolationKind::UnsupportedFact,
                message: "Every source is a note the developer labelled hypothesis, question, or next step, so this cannot be a verified fact. Move it to hypotheses or unresolved_questions.".into(),
            });
        }
    }

    duplicate_ids(
        "verified_facts",
        capsule.verified_facts.iter().map(|f| f.id.as_str()),
        &mut violations,
    );
    duplicate_ids(
        "hypotheses",
        capsule.hypotheses.iter().map(|h| h.id.as_str()),
        &mut violations,
    );

    for (index, artifact) in capsule.relevant_artifacts.iter().enumerate() {
        if let Err(reason) = check_artifact_path(&artifact.relative_path, &context.boundary) {
            violations.push(Violation {
                path: format!("/relevant_artifacts/{index}/relative_path"),
                kind: ViolationKind::ArtifactPath,
                message: format!("{} {reason}", clip(&artifact.relative_path, 80)),
            });
        }
    }

    // Anything that looks like a credential, wherever it appears.
    if let Ok(value) = serde_json::to_value(capsule) {
        find_credentials(context.redactor, &value, String::new(), &mut violations);
    }

    violations.sort();
    violations.dedup();
    violations
}

fn duplicate_ids<'a>(
    section: &str,
    ids: impl Iterator<Item = &'a str>,
    violations: &mut Vec<Violation>,
) {
    let mut seen: HashSet<&str> = HashSet::new();
    for (index, id) in ids.enumerate() {
        if !seen.insert(id) {
            violations.push(Violation {
                path: format!("/{section}/{index}/id"),
                kind: ViolationKind::DuplicateId,
                message: format!(
                    "The id {} is used more than once in {section}.",
                    clip(id, 60)
                ),
            });
        }
    }
}

/// An artifact path must be relative, stay inside the project, and not be a
/// path ThreadBack never captures.
fn check_artifact_path(path: &str, boundary: &ProjectBoundary) -> Result<(), String> {
    let bytes = path.as_bytes();
    let drive = bytes.len() > 1 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if path.starts_with('/') || path.starts_with('~') || path.starts_with("\\\\") || drive {
        return Err("is an absolute path. Use a path relative to the project.".into());
    }
    if path.contains('\\') {
        return Err("contains a backslash. Use forward slashes.".into());
    }
    if path.contains('\0') || path != path.trim() {
        return Err("contains stray whitespace or control characters.".into());
    }
    if path.split('/').any(|part| part == "..") {
        return Err("goes up out of its folder with '..'.".into());
    }
    let resolved = boundary
        .resolve(Path::new(path))
        .map_err(|error| format!("cannot be used: {}", error.message.trim_end_matches('.')))?;
    if resolved.relative == "." {
        return Err("is the project folder itself, not a file in it.".into());
    }
    Ok(())
}

fn find_credentials(redactor: &Redactor, value: &Value, path: String, out: &mut Vec<Violation>) {
    match value {
        Value::String(text) => {
            let findings = redactor.find_secrets(text);
            if !findings.is_empty() {
                let what: Vec<&str> = findings.iter().map(|f| f.kind.description()).collect();
                out.push(Violation {
                    path,
                    kind: ViolationKind::Credential,
                    message: format!(
                        "Contains {}. Remove it; never reproduce credentials.",
                        what.join(", ")
                    ),
                });
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                find_credentials(redactor, item, format!("{path}/{index}"), out);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                find_credentials(redactor, item, format!("{path}/{key}"), out);
            }
        }
        _ => {}
    }
}

/// Shortens model-supplied text before it is shown or sent back, on one line.
pub(crate) fn clip(text: &str, max: usize) -> String {
    let single: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if single.chars().count() <= max {
        single
    } else {
        format!("{}…", single.chars().take(max).collect::<String>())
    }
}

impl From<ViolationKind> for &'static str {
    fn from(kind: ViolationKind) -> Self {
        match kind {
            ViolationKind::NotJson => "not-json",
            ViolationKind::Shape => "shape",
            ViolationKind::Provenance => "provenance",
            ViolationKind::UnsupportedFact => "unsupported-fact",
            ViolationKind::DuplicateId => "duplicate-id",
            ViolationKind::ArtifactPath => "artifact-path",
            ViolationKind::Credential => "credential",
        }
    }
}

/// A short account of the first problems, for an error message.
pub fn summarize(violations: &[Violation], limit: usize) -> String {
    let mut parts: Vec<String> = violations
        .iter()
        .take(limit)
        .map(|v| {
            if v.path.is_empty() {
                clip(&v.message, 160)
            } else {
                format!("{}: {}", v.path, clip(&v.message, 160))
            }
        })
        .collect();
    if violations.len() > limit {
        parts.push(format!("and {} more", violations.len() - limit));
    }
    parts.join("; ")
}
