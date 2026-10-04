use super::prompt::PromptRequest;
use super::repair::build_repair_prompt;
use super::validate::{Evaluation, ValidationContext, Violation, evaluate, summarize};
use crate::{DomainError, ResumeCapsule};

/// What a model returned for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelReply {
    pub text: String,
}

/// The seam between validation and a real model. The Token Factory client
/// implements it; tests script it. It receives the whole request, so it can
/// never be given anything the user did not review.
pub trait CapsuleModel {
    fn complete(&self, request: &PromptRequest) -> Result<ModelReply, DomainError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedCapsule {
    pub capsule: ResumeCapsule,
    /// 1 when the first reply was valid, 2 when it took the correction.
    pub attempts: u8,
    /// The problems with the first reply, when a correction was needed.
    pub first_violations: Vec<Violation>,
}

impl GeneratedCapsule {
    pub fn repaired(&self) -> bool {
        self.attempts > 1
    }
}

/// Both replies failed. The reasons for each are kept so they can be shown.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidOutput {
    pub first: Vec<Violation>,
    pub second: Vec<Violation>,
    /// The last reply, for local inspection only.
    pub last_reply: String,
}

#[derive(Debug)]
pub enum GenerationFailure {
    /// The model could not be reached or refused; no correction is attempted.
    Model(DomainError),
    /// Both the reply and its one correction failed validation.
    InvalidOutput(InvalidOutput),
}

impl From<GenerationFailure> for DomainError {
    fn from(failure: GenerationFailure) -> Self {
        match failure {
            GenerationFailure::Model(error) => error,
            GenerationFailure::InvalidOutput(invalid) => {
                DomainError::invalid_model_output(format!(
                    "The model's reply did not meet the capsule contract, even after one correction. \
Your checkpoint is kept and nothing was approved. You can try again. Problems: {}",
                    summarize(&invalid.second, 3)
                ))
            }
        }
    }
}

/// Asks the model for a capsule and judges the reply. A reply that fails gets
/// exactly one correction request; a second failure is returned as an error
/// and no capsule is produced, so an unsupported verified fact can never
/// reach the review step. At most two model calls are ever made.
pub fn generate_capsule(
    model: &dyn CapsuleModel,
    request: &PromptRequest,
    context: &ValidationContext,
) -> Result<GeneratedCapsule, GenerationFailure> {
    let first = model.complete(request).map_err(GenerationFailure::Model)?;
    let first_violations = match evaluate(&first.text, context) {
        Evaluation::Valid(capsule) => {
            return Ok(GeneratedCapsule {
                capsule: *capsule,
                attempts: 1,
                first_violations: Vec::new(),
            });
        }
        Evaluation::Invalid(violations) => violations,
    };

    let repair = build_repair_prompt(request, &first.text, &first_violations);
    let second = model.complete(&repair).map_err(GenerationFailure::Model)?;
    match evaluate(&second.text, context) {
        Evaluation::Valid(capsule) => Ok(GeneratedCapsule {
            capsule: *capsule,
            attempts: 2,
            first_violations,
        }),
        Evaluation::Invalid(second_violations) => {
            Err(GenerationFailure::InvalidOutput(InvalidOutput {
                first: first_violations,
                second: second_violations,
                last_reply: second.text,
            }))
        }
    }
}
