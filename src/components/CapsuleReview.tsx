import { forwardRef, useEffect, useRef, useState } from "react";
import type {
  ApprovalResult,
  Checkpoint,
  CheckpointEntry,
  CheckpointPayload,
  Confidence,
  ResumeCapsule,
} from "../bindings/contracts";
import { checkpoints as checkpointsApi } from "../api/commands";
import { toDomainError } from "../api/errors";
import {
  cloneCapsule,
  factProblems,
  factToHypothesis,
  fieldId,
  hypothesisToFact,
  removeAt,
  replaceAt,
  sameCapsule,
  supportProblem,
  toggleSource,
} from "../lib/capsuleEdit";
import { pluralize, summarizeEntry } from "../lib/format";
import { CapsuleView } from "./CapsuleView";
import { SourceList } from "./SourceList";

interface CapsuleReviewProps {
  checkpoint: Checkpoint;
  /** Another action is running elsewhere on the screen. */
  busy: boolean;
  onApproved: (result: ApprovalResult) => Promise<void> | void;
  /** Throws this draft away, saves the place again, and returns to the payload review. */
  onRegenerate: () => Promise<void>;
}

interface FieldError {
  message: string;
  field: string | null;
}

const CONFIDENCE_OPTIONS: { value: Confidence; label: string }[] = [
  { value: "low", label: "Low confidence" },
  { value: "medium", label: "Medium confidence" },
  { value: "high", label: "High confidence" },
];

interface TextProps {
  path: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  error: FieldError | null;
  rows?: number;
}

function TextField({ path, label, value, onChange, error, rows = 2 }: TextProps) {
  const id = fieldId(path);
  const invalid = error?.field === path;
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <textarea
        id={id}
        rows={rows}
        value={value}
        aria-invalid={invalid || undefined}
        aria-describedby={invalid ? "capsule-error" : undefined}
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  );
}

interface SourceEditorProps {
  path: string;
  label: string;
  ids: string[];
  entries: CheckpointEntry[];
  onChange: (ids: string[]) => void;
  error: FieldError | null;
}

/** Shows the cited evidence and lets the user change which checkpoint entries it cites. */
function SourceEditor({ path, label, ids, entries, onChange, error }: SourceEditorProps) {
  const [editing, setEditing] = useState(false);
  const known = new Set(entries.map((entry) => entry.eventId));
  const unknown = ids.filter((id) => !known.has(id));
  const invalid = error?.field === path;
  return (
    <div className="source-editor">
      <SourceList ids={ids} entries={entries} />
      <button
        type="button"
        aria-expanded={editing}
        aria-label={`${editing ? "Done editing" : "Edit"} sources for ${label}`}
        onClick={() => setEditing(!editing)}
      >
        {editing ? "Done with sources" : "Edit sources"}
      </button>
      {editing || invalid ? (
        <fieldset id={fieldId(path)} tabIndex={-1} aria-invalid={invalid || undefined} className="source-picker">
          <legend>Sources for {label}</legend>
          {unknown.map((id) => (
            <label key={id} className="check">
              <input type="checkbox" checked onChange={() => onChange(toggleSource(ids, id))} />
              <span>
                <span aria-hidden="true">✕ </span>Unknown event (not in this checkpoint)
              </span>
            </label>
          ))}
          {entries.map((entry) => {
            const summary = summarizeEntry(entry);
            const text = summary.text.length > 90 ? `${summary.text.slice(0, 90)}…` : summary.text;
            return (
              <label key={entry.eventId} className="check">
                <input
                  type="checkbox"
                  checked={ids.includes(entry.eventId)}
                  onChange={() => onChange(toggleSource(ids, entry.eventId))}
                />
                <span>
                  {summary.kind}: {text}
                </span>
              </label>
            );
          })}
        </fieldset>
      ) : null}
    </div>
  );
}

function AddText({ label, onAdd }: { label: string; onAdd: (text: string) => void }) {
  const [text, setText] = useState("");
  return (
    <div className="add-row">
      <label>
        <span className="visually-hidden">{label}</span>
        <input type="text" value={text} placeholder={label} onChange={(event) => setText(event.target.value)} />
      </label>
      <button
        type="button"
        disabled={!text.trim()}
        onClick={() => {
          onAdd(text.trim());
          setText("");
        }}
      >
        Add
      </button>
    </div>
  );
}

/**
 * The model's draft as the user's working copy: every claim can be edited,
 * re-sourced, relabelled, or removed before approval. The draft itself is never
 * changed, and approval sends the whole working copy for the server to check.
 */
export const CapsuleReview = forwardRef<HTMLHeadingElement, CapsuleReviewProps>(function CapsuleReview(
  { checkpoint, busy, onApproved, onRegenerate },
  headingRef,
) {
  const draft = checkpoint.draft as ResumeCapsule;
  const entries = (checkpoint.payload as unknown as CheckpointPayload).entries;
  const [work, setWork] = useState<ResumeCapsule>(() => cloneCapsule(draft));
  const [approving, setApproving] = useState(false);
  const [error, setError] = useState<FieldError | null>(null);
  const [status, setStatus] = useState("");
  const [showOriginal, setShowOriginal] = useState(false);
  const [confirmingRegenerate, setConfirmingRegenerate] = useState(false);
  const errorRef = useRef<HTMLParagraphElement>(null);
  const regenerateConfirm = useRef<HTMLButtonElement>(null);

  const changed = !sameCapsule(work, draft);
  const problems = factProblems(work, entries);
  const blocked = problems.length > 0;
  const disabled = busy || approving;

  const update = (patch: Partial<ResumeCapsule>) => {
    setError(null);
    setWork((current) => ({ ...current, ...patch }));
  };

  useEffect(() => {
    if (!error) return;
    const target = error.field ? document.getElementById(fieldId(error.field)) : null;
    (target ?? errorRef.current)?.focus();
  }, [error]);

  useEffect(() => {
    if (confirmingRegenerate) regenerateConfirm.current?.focus();
  }, [confirmingRegenerate]);

  async function approve() {
    if (blocked || disabled) return;
    setApproving(true);
    setError(null);
    try {
      const result = await checkpointsApi.approve(checkpoint.id, work);
      await onApproved(result);
    } catch (failure) {
      const domain = toDomainError(failure);
      setError({ message: domain.message, field: domain.field });
    } finally {
      setApproving(false);
    }
  }

  return (
    <section className="panel capsule" aria-labelledby="capsule-title">
      <h2 id="capsule-title" tabIndex={-1} ref={headingRef}>
        Resume capsule (draft)
      </h2>
      <p className="hint">
        Written by {checkpoint.modelId ?? "the model"} from the evidence you approved
        {checkpoint.promptVersion ? ` (prompt ${checkpoint.promptVersion})` : ""}. It is a draft: correct anything
        that is wrong, then approve it. Your edits are authoritative, and the model's original stays available.
      </p>
      <p className="visually-hidden" role="status" aria-live="polite">
        {status}
      </p>
      <p className="hint">
        <span aria-hidden="true">{changed ? "✎ " : "• "}</span>
        {changed
          ? "You have edits that are kept only in this window until you approve."
          : "No changes yet. Approving now records no corrections."}
      </p>
      {error ? (
        <p id="capsule-error" className="form-error" role="alert" tabIndex={-1} ref={errorRef}>
          <span aria-hidden="true">⚠ </span>
          {error.message}
        </p>
      ) : null}

      <section aria-labelledby="review-next-title" className="capsule-next">
        <h3 id="review-next-title">Next action</h3>
        <TextField path="/next_action/instruction" label="Next action instruction" value={work.next_action.instruction} error={error} onChange={(value) => update({ next_action: { ...work.next_action, instruction: value } })} />
        <TextField path="/next_action/justification" label="Why this is the next action" value={work.next_action.justification} error={error} onChange={(value) => update({ next_action: { ...work.next_action, justification: value } })} />
        <TextField path="/next_action/success_check" label="How you will know it worked" value={work.next_action.success_check} error={error} onChange={(value) => update({ next_action: { ...work.next_action, success_check: value } })} />
        <SourceEditor path="/next_action/source_event_ids" label="the next action" ids={work.next_action.source_event_ids} entries={entries} error={error} onChange={(ids) => update({ next_action: { ...work.next_action, source_event_ids: ids } })} />
      </section>

      <section aria-labelledby="review-summary-title">
        <h3 id="review-summary-title">Where things stand</h3>
        <TextField path="/checkpoint_summary" label="Summary" rows={3} value={work.checkpoint_summary} error={error} onChange={(value) => update({ checkpoint_summary: value })} />
        <TextField path="/goal" label="Goal" value={work.goal} error={error} onChange={(value) => update({ goal: value })} />
      </section>

      <section aria-labelledby="review-facts-title">
        <h3 id="review-facts-title">Verified facts</h3>
        {blocked ? (
          <p className="form-error" role="status">
            <span aria-hidden="true">⚠ </span>
            {pluralize(problems.length, "fact needs", "facts need")} review before this can be approved. Fix its
            sources, make it a hypothesis, or remove it.
          </p>
        ) : null}
        {work.verified_facts.length === 0 ? (
          <p className="empty">No claim in this capsule is a verified fact.</p>
        ) : (
          <ul className="plain edit-list">
            {work.verified_facts.map((fact, index) => {
              const problem = problems.find((candidate) => candidate.index === index);
              const name = `fact ${index + 1}`;
              return (
                <li key={fact.id}>
                  <p>
                    {problem ? (
                      <>
                        <span aria-hidden="true">⚠ </span>
                        <strong>Needs review.</strong> <span className="hint">{problem.message}</span>
                      </>
                    ) : (
                      <>
                        <span aria-hidden="true">✓ </span>
                        <strong>Verified</strong>
                      </>
                    )}
                  </p>
                  <TextField path={`/verified_facts/${index}/claim`} label={`Claim for ${name}`} value={fact.claim} error={error} onChange={(value) => update({ verified_facts: replaceAt(work.verified_facts, index, { claim: value }) })} />
                  <SourceEditor path={`/verified_facts/${index}/source_event_ids`} label={name} ids={fact.source_event_ids} entries={entries} error={error} onChange={(ids) => update({ verified_facts: replaceAt(work.verified_facts, index, { source_event_ids: ids }) })} />
                  <div className="actions" role="group" aria-label={`Actions for ${name}`}>
                    <button
                      type="button"
                      onClick={() => {
                        setError(null);
                        setWork(factToHypothesis(work, index, "low"));
                        setStatus(`Moved ${name} to hypotheses as low confidence. Change the confidence there if needed.`);
                      }}
                    >
                      Make {name} a hypothesis
                    </button>
                    <button type="button" onClick={() => { update({ verified_facts: removeAt(work.verified_facts, index) }); setStatus(`Removed ${name}.`); }}>
                      Remove {name}
                    </button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section aria-labelledby="review-hypotheses-title">
        <h3 id="review-hypotheses-title">Hypotheses (not proven)</h3>
        {work.hypotheses.length === 0 ? (
          <p className="empty">None.</p>
        ) : (
          <ul className="plain edit-list">
            {work.hypotheses.map((hypothesis, index) => {
              const name = `hypothesis ${index + 1}`;
              const why = supportProblem(hypothesis.source_event_ids, entries);
              return (
                <li key={hypothesis.id}>
                  <p>
                    <span aria-hidden="true">? </span>
                    <strong>Hypothesis</strong>
                  </p>
                  <TextField path={`/hypotheses/${index}/claim`} label={`Claim for ${name}`} value={hypothesis.claim} error={error} onChange={(value) => update({ hypotheses: replaceAt(work.hypotheses, index, { claim: value }) })} />
                  <div className="field">
                    <label htmlFor={fieldId(`/hypotheses/${index}/confidence`)}>Confidence for {name}</label>
                    <select
                      id={fieldId(`/hypotheses/${index}/confidence`)}
                      value={hypothesis.confidence}
                      onChange={(event) => update({ hypotheses: replaceAt(work.hypotheses, index, { confidence: event.target.value as Confidence }) })}
                    >
                      {CONFIDENCE_OPTIONS.map((option) => (
                        <option key={option.value} value={option.value}>
                          {option.label}
                        </option>
                      ))}
                    </select>
                  </div>
                  <SourceEditor path={`/hypotheses/${index}/source_event_ids`} label={name} ids={hypothesis.source_event_ids} entries={entries} error={error} onChange={(ids) => update({ hypotheses: replaceAt(work.hypotheses, index, { source_event_ids: ids }) })} />
                  <div className="actions" role="group" aria-label={`Actions for ${name}`}>
                    <button
                      type="button"
                      aria-disabled={why !== null}
                      aria-describedby={why ? `why-${index}` : undefined}
                      onClick={() => {
                        if (why) return;
                        setError(null);
                        setWork(hypothesisToFact(work, index));
                        setStatus(`Moved ${name} to verified facts.`);
                      }}
                    >
                      Make {name} a verified fact
                    </button>
                    <button type="button" onClick={() => { update({ hypotheses: removeAt(work.hypotheses, index) }); setStatus(`Removed ${name}.`); }}>
                      Remove {name}
                    </button>
                  </div>
                  {why ? (
                    <p id={`why-${index}`} className="hint">
                      Cannot be a verified fact yet: {why} Add a source that can support it first.
                    </p>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section aria-labelledby="review-attempts-title">
        <h3 id="review-attempts-title">Already tried</h3>
        {work.failed_attempts.length === 0 ? (
          <p className="empty">Nothing recorded.</p>
        ) : (
          <ul className="plain edit-list">
            {work.failed_attempts.map((attempt, index) => {
              const name = `attempt ${index + 1}`;
              const set = (change: Partial<typeof attempt>) => update({ failed_attempts: replaceAt(work.failed_attempts, index, change) });
              return (
                <li key={index}>
                  <TextField path={`/failed_attempts/${index}/action`} label={`What was tried in ${name}`} value={attempt.action} error={error} onChange={(value) => set({ action: value })} />
                  <TextField path={`/failed_attempts/${index}/outcome`} label={`Outcome of ${name}`} value={attempt.outcome} error={error} onChange={(value) => set({ outcome: value })} />
                  <TextField path={`/failed_attempts/${index}/learning`} label={`What ${name} showed`} value={attempt.learning} error={error} onChange={(value) => set({ learning: value })} />
                  <SourceEditor path={`/failed_attempts/${index}/source_event_ids`} label={name} ids={attempt.source_event_ids} entries={entries} error={error} onChange={(ids) => set({ source_event_ids: ids })} />
                  <button type="button" onClick={() => { update({ failed_attempts: removeAt(work.failed_attempts, index) }); setStatus(`Removed ${name}.`); }}>
                    Remove {name}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section aria-labelledby="review-questions-title">
        <h3 id="review-questions-title">Still unknown</h3>
        {work.unresolved_questions.length === 0 ? <p className="empty">No open questions.</p> : null}
        <ul className="plain edit-list">
          {work.unresolved_questions.map((question, index) => (
            <li key={index}>
              <TextField path={`/unresolved_questions/${index}`} label={`Open question ${index + 1}`} value={question} error={error} onChange={(value) => update({ unresolved_questions: work.unresolved_questions.map((q, i) => (i === index ? value : q)) })} />
              <button type="button" onClick={() => { update({ unresolved_questions: removeAt(work.unresolved_questions, index) }); setStatus(`Removed open question ${index + 1}.`); }}>
                Remove open question {index + 1}
              </button>
            </li>
          ))}
        </ul>
        <AddText label="Add an open question" onAdd={(text) => update({ unresolved_questions: [...work.unresolved_questions, text] })} />
      </section>

      <section aria-labelledby="review-artifacts-title">
        <h3 id="review-artifacts-title">Files that matter</h3>
        {work.relevant_artifacts.length === 0 ? (
          <p className="empty">None named.</p>
        ) : (
          <ul className="plain edit-list">
            {work.relevant_artifacts.map((artifact, index) => {
              const name = `file ${index + 1}`;
              return (
                <li key={artifact.relative_path}>
                  <p>
                    <code>{artifact.relative_path}</code> <span className="hint">({artifact.type})</span>
                  </p>
                  <TextField path={`/relevant_artifacts/${index}/reason`} label={`Why ${name} matters`} value={artifact.reason} error={error} onChange={(value) => update({ relevant_artifacts: replaceAt(work.relevant_artifacts, index, { reason: value }) })} />
                  <SourceEditor path={`/relevant_artifacts/${index}/source_event_ids`} label={name} ids={artifact.source_event_ids} entries={entries} error={error} onChange={(ids) => update({ relevant_artifacts: replaceAt(work.relevant_artifacts, index, { source_event_ids: ids }) })} />
                  <button type="button" onClick={() => { update({ relevant_artifacts: removeAt(work.relevant_artifacts, index) }); setStatus(`Removed ${name}.`); }}>
                    Remove {name}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section aria-labelledby="review-limits-title">
        <h3 id="review-limits-title">Limits of this capsule</h3>
        {work.limitations.length === 0 ? <p className="empty">None stated.</p> : null}
        <ul className="plain edit-list">
          {work.limitations.map((limit, index) => (
            <li key={index}>
              <TextField path={`/limitations/${index}`} label={`Limit ${index + 1}`} value={limit} error={error} onChange={(value) => update({ limitations: work.limitations.map((l, i) => (i === index ? value : l)) })} />
              <button type="button" onClick={() => { update({ limitations: removeAt(work.limitations, index) }); setStatus(`Removed limit ${index + 1}.`); }}>
                Remove limit {index + 1}
              </button>
            </li>
          ))}
        </ul>
        <AddText label="Add a limit" onAdd={(text) => update({ limitations: [...work.limitations, text] })} />
      </section>

      <div className="actions" role="group" aria-label="Review actions">
        <button
          type="button"
          className="primary"
          aria-disabled={blocked || disabled}
          aria-busy={approving}
          aria-describedby={blocked ? "approve-why" : undefined}
          onClick={approve}
        >
          {approving ? "Approving…" : "Approve capsule"}
        </button>
        <button
          type="button"
          disabled={disabled || !changed}
          onClick={() => {
            setWork(cloneCapsule(draft));
            setError(null);
            setStatus("Your edits were discarded. The model's draft is back.");
          }}
        >
          Discard my edits
        </button>
        <button type="button" aria-expanded={showOriginal} onClick={() => setShowOriginal(!showOriginal)}>
          {showOriginal ? "Hide the original model draft" : "Show the original model draft"}
        </button>
        <button type="button" disabled={disabled} onClick={() => setConfirmingRegenerate(true)}>
          Regenerate…
        </button>
      </div>
      {blocked ? (
        <p id="approve-why" className="hint">
          Approval is unavailable while {pluralize(problems.length, "fact is", "facts are")} marked Needs review.
        </p>
      ) : null}

      {confirmingRegenerate ? (
        <div className="confirm panel" role="group" aria-label="Confirm regenerating the capsule">
          <p>
            Regenerate throws away this draft and your edits, saves your place again, and takes you to the payload
            review. Nothing is sent until you confirm there.
          </p>
          <div className="actions">
            <button
              type="button"
              className="primary"
              ref={regenerateConfirm}
              disabled={disabled}
              onClick={() => {
                setConfirmingRegenerate(false);
                void onRegenerate();
              }}
            >
              Discard and regenerate
            </button>
            <button type="button" onClick={() => setConfirmingRegenerate(false)}>
              Keep this draft
            </button>
          </div>
        </div>
      ) : null}

      {showOriginal ? (
        <CapsuleView
          checkpoint={checkpoint}
          capsule={draft}
          title="Original model draft"
          idPrefix="original"
          headingLevel={3}
          intro={<p className="hint">Exactly as the model returned it. It is read-only and never changes.</p>}
        />
      ) : null}
    </section>
  );
});
