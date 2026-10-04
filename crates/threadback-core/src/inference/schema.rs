use jsonschema::Validator;
use serde_json::{Value, json};

use crate::DomainError;

/// The version of the capsule contract. A response with any other version is
/// rejected, so a future schema cannot be mistaken for this one.
pub const SCHEMA_VERSION: &str = "1.0";

// Bounds keep a capsule readable and a runaway response from filling storage.
const SHORT_TEXT: u64 = 1_000;
const LONG_TEXT: u64 = 1_500;
const MAX_ID: u64 = 64;
const MAX_PATH: u64 = 300;
const MAX_SOURCES: u64 = 20;

fn text(max: u64) -> Value {
    json!({ "type": "string", "minLength": 1, "maxLength": max })
}

fn sources(min: u64) -> Value {
    json!({
        "type": "array",
        "minItems": min,
        "maxItems": MAX_SOURCES,
        "items": { "type": "string", "minLength": 1, "maxLength": MAX_ID }
    })
}

fn list(item: Value, max: u64) -> Value {
    json!({ "type": "array", "maxItems": max, "items": item })
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

/// The resume capsule of `spec.md` §11 as a JSON Schema. Every object rejects
/// unknown properties, and every section is required, so the shape cannot be
/// changed by anything the model was shown.
pub fn capsule_schema() -> Value {
    let verified_fact = object(
        json!({
            "id": text(MAX_ID),
            "claim": text(SHORT_TEXT),
            "source_event_ids": sources(1),
        }),
        &["id", "claim", "source_event_ids"],
    );
    let hypothesis = object(
        json!({
            "id": text(MAX_ID),
            "claim": text(SHORT_TEXT),
            "confidence": { "type": "string", "enum": ["low", "medium", "high"] },
            "source_event_ids": sources(0),
        }),
        &["id", "claim", "confidence", "source_event_ids"],
    );
    let failed_attempt = object(
        json!({
            "action": text(SHORT_TEXT),
            "outcome": text(SHORT_TEXT),
            "learning": text(SHORT_TEXT),
            "source_event_ids": sources(1),
        }),
        &["action", "outcome", "learning", "source_event_ids"],
    );
    let artifact = object(
        json!({
            "type": text(MAX_ID),
            "relative_path": text(MAX_PATH),
            "reason": text(SHORT_TEXT),
            "source_event_ids": sources(0),
        }),
        &["type", "relative_path", "reason", "source_event_ids"],
    );
    let next_action = object(
        json!({
            "instruction": text(SHORT_TEXT),
            "justification": text(SHORT_TEXT),
            "success_check": text(SHORT_TEXT),
            "source_event_ids": sources(0),
        }),
        &[
            "instruction",
            "justification",
            "success_check",
            "source_event_ids",
        ],
    );

    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "ThreadBack resume capsule",
        "type": "object",
        "properties": {
            "schema_version": { "const": SCHEMA_VERSION },
            "goal": text(LONG_TEXT),
            "checkpoint_summary": text(LONG_TEXT),
            "verified_facts": list(verified_fact, 20),
            "hypotheses": list(hypothesis, 10),
            "failed_attempts": list(failed_attempt, 15),
            "unresolved_questions": list(text(SHORT_TEXT), 10),
            "relevant_artifacts": list(artifact, 15),
            "next_action": next_action,
            "limitations": list(text(SHORT_TEXT), 10),
        },
        "required": [
            "schema_version", "goal", "checkpoint_summary", "verified_facts",
            "hypotheses", "failed_attempts", "unresolved_questions",
            "relevant_artifacts", "next_action", "limitations"
        ],
        "additionalProperties": false
    })
}

/// One reason a response does not match the schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIssue {
    /// Where in the response, such as `/verified_facts/0/source_event_ids`.
    pub path: String,
    pub message: String,
}

pub struct CapsuleSchema {
    validator: Validator,
}

impl CapsuleSchema {
    pub fn new() -> Result<Self, DomainError> {
        let validator = jsonschema::validator_for(&capsule_schema()).map_err(|error| {
            DomainError::internal(format!("The capsule schema is invalid: {error}"))
        })?;
        Ok(Self { validator })
    }

    /// Every way `candidate` fails the schema, in a stable order; empty when it
    /// matches.
    pub fn check(&self, candidate: &Value) -> Vec<SchemaIssue> {
        let mut issues: Vec<SchemaIssue> = self
            .validator
            .iter_errors(candidate)
            .map(|error| SchemaIssue {
                path: error.instance_path().to_string(),
                message: error.to_string(),
            })
            .collect();
        issues.sort_by(|a, b| a.path.cmp(&b.path).then(a.message.cmp(&b.message)));
        issues
    }

    pub fn is_valid(&self, candidate: &Value) -> bool {
        self.validator.is_valid(candidate)
    }
}
