pub mod diagnostics;
pub mod generate;
pub mod generation;
pub mod prompt;
pub mod repair;
pub mod schema;
#[cfg(feature = "model-client")]
pub mod token_factory;
pub mod validate;

pub use diagnostics::ModelCallRecord;
pub use generate::{
    CapsuleModel, GeneratedCapsule, GenerationFailure, InvalidOutput, ModelReply, generate_capsule,
};
pub use generation::{
    GenerationPlan, finish_generation, generate_draft, plan_generation, run_generation,
};
pub use prompt::{ChatMessage, PROMPT_VERSION, PromptRequest, build_prompt};
pub use repair::build_repair_prompt;
pub use schema::{CapsuleSchema, SCHEMA_VERSION, SchemaIssue, capsule_schema};
#[cfg(feature = "model-client")]
pub use token_factory::{CancelToken, TokenFactoryClient};
pub use validate::{
    Evaluation, MAX_VIOLATIONS, ShownEntry, ValidationContext, Violation, ViolationKind, evaluate,
    summarize, validate_capsule,
};
