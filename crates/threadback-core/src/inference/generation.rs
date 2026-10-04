use serde_json::json;

use super::{
    diagnostics::ModelCallRecord,
    generate::{CapsuleModel, GeneratedCapsule, GenerationFailure, generate_capsule},
    prompt::{PROMPT_VERSION, PromptRequest, build_prompt},
    schema::SCHEMA_VERSION,
    validate::ValidationContext,
};
use crate::{
    Checkpoint, CheckpointId, Database, DomainError, EventId, EventPayload, OutboundService,
    ProjectBoundary, Redactor, STATUS_GENERATED, Sensitivity, ThreadEvent,
};

/// Everything decided before the model is called. Building it needs the
/// database; calling the model does not, so a slow request never holds the
/// database lock.
pub struct GenerationPlan {
    pub checkpoint_id: CheckpointId,
    pub request: PromptRequest,
    pub payload_hash: String,
    context: ValidationContext,
}

/// Phase 1. Authorizes the reviewed payload (a blocked, empty, or changed one
/// is refused here), builds the prompt, and prepares the checks the reply will
/// be held to. Nothing is sent.
pub fn plan_generation(
    database: &Database,
    checkpoint_id: &CheckpointId,
    excluded: &[EventId],
    reviewed_hash: &str,
    redactor: &Redactor,
) -> Result<GenerationPlan, DomainError> {
    let checkpoint = database
        .find_checkpoint(checkpoint_id)?
        .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
    if checkpoint.draft.is_some() || checkpoint.status == STATUS_GENERATED {
        return Err(DomainError::invalid_state(
            "A capsule was already generated for this draft. Discard the draft to start again.",
        ));
    }

    let outbound = OutboundService::with_redactor(database, redactor);
    let approved = outbound.authorize(checkpoint_id, excluded, reviewed_hash)?;
    let request = build_prompt(&approved)?;

    let thread = database
        .find_thread(&checkpoint.thread_id)?
        .ok_or_else(|| DomainError::not_found("That thread no longer exists."))?;
    let project = database
        .find_project(&thread.project_id)?
        .ok_or_else(|| DomainError::not_found("The thread's project is no longer approved."))?;
    let boundary = ProjectBoundary::new(&project.canonical_root, &[])?;
    let context = outbound.validation_context(&approved, boundary)?;

    Ok(GenerationPlan {
        checkpoint_id: checkpoint_id.clone(),
        payload_hash: approved.hash().to_owned(),
        request,
        context,
    })
}

/// Phase 2. Asks the model, judges the reply, and allows one correction. It
/// touches no database.
pub fn run_generation(
    model: &dyn CapsuleModel,
    plan: &GenerationPlan,
) -> Result<GeneratedCapsule, GenerationFailure> {
    generate_capsule(model, &plan.request, &plan.context)
}

/// Phase 3. Records what was asked of the model (always), and if a valid
/// capsule came back, stores it as the checkpoint's draft and notes in the
/// thread's history that it was generated, with the model, prompt version,
/// and payload fingerprint but none of the content.
pub fn finish_generation(
    database: &Database,
    plan: &GenerationPlan,
    model_id: &str,
    outcome: Result<GeneratedCapsule, GenerationFailure>,
    calls: &[ModelCallRecord],
) -> Result<Checkpoint, DomainError> {
    for call in calls {
        database.record_model_call(Some(&plan.checkpoint_id), call)?;
    }
    let generated = outcome.map_err(DomainError::from)?;

    database.with_transaction(|db| {
        let checkpoint = db
            .find_checkpoint(&plan.checkpoint_id)?
            .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))?;
        db.set_checkpoint_draft(
            &plan.checkpoint_id,
            &generated.capsule,
            model_id,
            PROMPT_VERSION,
        )?;

        let sum = |pick: fn(&ModelCallRecord) -> Option<u32>| -> u64 {
            calls.iter().filter_map(pick).map(u64::from).sum()
        };
        let metadata = json!({
            "checkpointId": plan.checkpoint_id,
            "modelId": model_id,
            "promptVersion": PROMPT_VERSION,
            "schemaVersion": SCHEMA_VERSION,
            "payloadHash": plan.payload_hash,
            "attempts": generated.attempts,
            "repaired": generated.repaired(),
            "latencyMs": calls.iter().map(|c| c.latency_ms).sum::<u64>(),
            "promptTokens": sum(|c| c.prompt_tokens),
            "completionTokens": sum(|c| c.completion_tokens),
        });
        db.insert_event(&ThreadEvent::new(
            checkpoint.thread_id,
            EventPayload::CheckpointGenerated(metadata),
            Sensitivity::Normal,
        )?)?;
        Ok(())
    })?;

    database
        .find_checkpoint(&plan.checkpoint_id)?
        .ok_or_else(|| DomainError::not_found("That checkpoint no longer exists."))
}

/// All three phases, for callers that can hold the database throughout.
pub fn generate_draft(
    database: &Database,
    checkpoint_id: &CheckpointId,
    excluded: &[EventId],
    reviewed_hash: &str,
    redactor: &Redactor,
    model: &dyn CapsuleModel,
    model_id: &str,
) -> Result<Checkpoint, DomainError> {
    let plan = plan_generation(database, checkpoint_id, excluded, reviewed_hash, redactor)?;
    let outcome = run_generation(model, &plan);
    let calls = model.take_diagnostics();
    finish_generation(database, &plan, model_id, outcome, &calls)
}
