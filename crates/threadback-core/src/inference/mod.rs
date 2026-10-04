pub mod generate;
pub mod prompt;
pub mod repair;
pub mod schema;
pub mod validate;

pub use generate::{
    CapsuleModel, GeneratedCapsule, GenerationFailure, InvalidOutput, ModelReply, generate_capsule,
};
pub use prompt::{ChatMessage, PROMPT_VERSION, PromptRequest, build_prompt};
pub use repair::build_repair_prompt;
pub use schema::{CapsuleSchema, SCHEMA_VERSION, SchemaIssue, capsule_schema};
pub use validate::{
    Evaluation, MAX_VIOLATIONS, ShownEntry, ValidationContext, Violation, ViolationKind, evaluate,
    summarize, validate_capsule,
};
