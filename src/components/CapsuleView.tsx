import { forwardRef } from "react";
import type { Checkpoint, ResumeCapsule } from "../bindings/contracts";
import { pluralize } from "../lib/format";

const sources = (ids: string[]) => (ids.length === 0 ? "no cited events" : pluralize(ids.length, "cited event"));

const CONFIDENCE: Record<string, string> = {
  low: "low confidence",
  medium: "medium confidence",
  high: "high confidence",
};

/**
 * The model's capsule, read-only. Facts, hypotheses, failed attempts, and
 * open questions stay in separate sections and are labelled in words, so how
 * sure the capsule is can be read without relying on colour.
 */
export const CapsuleView = forwardRef<HTMLHeadingElement, { checkpoint: Checkpoint }>(function CapsuleView(
  { checkpoint },
  headingRef,
) {
  const capsule = checkpoint.draft as ResumeCapsule | null;
  if (!capsule) return null;

  return (
    <section className="panel capsule" aria-labelledby="capsule-title">
      <h2 id="capsule-title" tabIndex={-1} ref={headingRef}>
        Resume capsule (draft)
      </h2>
      <p className="hint">
        Written by {checkpoint.modelId ?? "the model"} from the evidence you approved
        {checkpoint.promptVersion ? ` (prompt ${checkpoint.promptVersion})` : ""}. It is a draft: review and
        approval come next, and nothing here is final.
      </p>

      <section aria-labelledby="next-title" className="capsule-next">
        <h3 id="next-title">Next action</h3>
        <p className="preformatted">{capsule.next_action.instruction}</p>
        <p className="detail">
          <strong>Why:</strong> {capsule.next_action.justification}
        </p>
        <p className="detail">
          <strong>You will know it worked when:</strong> {capsule.next_action.success_check}
        </p>
        <p className="hint">Based on {sources(capsule.next_action.source_event_ids)}.</p>
      </section>

      <section aria-labelledby="summary-title">
        <h3 id="summary-title">Where things stand</h3>
        <p className="preformatted">{capsule.checkpoint_summary}</p>
        <p className="hint">Goal: {capsule.goal}</p>
      </section>

      <section aria-labelledby="facts-title">
        <h3 id="facts-title">Verified facts</h3>
        {capsule.verified_facts.length === 0 ? (
          <p className="empty">No claim in this capsule is backed by evidence strongly enough to call it a fact.</p>
        ) : (
          <ul className="plain">
            {capsule.verified_facts.map((fact) => (
              <li key={fact.id}>
                <span aria-hidden="true">✓ </span>
                <span className="visually-hidden">Verified: </span>
                {fact.claim} <span className="hint">({sources(fact.source_event_ids)})</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="hypotheses-title">
        <h3 id="hypotheses-title">Hypotheses (not proven)</h3>
        {capsule.hypotheses.length === 0 ? (
          <p className="empty">None.</p>
        ) : (
          <ul className="plain">
            {capsule.hypotheses.map((hypothesis) => (
              <li key={hypothesis.id}>
                <span aria-hidden="true">? </span>
                <span className="visually-hidden">Hypothesis: </span>
                {hypothesis.claim}{" "}
                <span className="hint">
                  ({CONFIDENCE[hypothesis.confidence] ?? hypothesis.confidence}; {sources(hypothesis.source_event_ids)})
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="attempts-title">
        <h3 id="attempts-title">Already tried</h3>
        {capsule.failed_attempts.length === 0 ? (
          <p className="empty">Nothing recorded.</p>
        ) : (
          <ul className="plain">
            {capsule.failed_attempts.map((attempt, index) => (
              <li key={index}>
                <strong>{attempt.action}</strong> — {attempt.outcome} <em>Learned: {attempt.learning}</em>{" "}
                <span className="hint">({sources(attempt.source_event_ids)})</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="questions-title">
        <h3 id="questions-title">Still unknown</h3>
        {capsule.unresolved_questions.length === 0 ? (
          <p className="empty">No open questions.</p>
        ) : (
          <ul className="plain">
            {capsule.unresolved_questions.map((question) => (
              <li key={question}>{question}</li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="artifacts-title">
        <h3 id="artifacts-title">Files that matter</h3>
        {capsule.relevant_artifacts.length === 0 ? (
          <p className="empty">None named.</p>
        ) : (
          <ul className="plain">
            {capsule.relevant_artifacts.map((artifact) => (
              <li key={artifact.relative_path}>
                <code>{artifact.relative_path}</code> <span className="hint">({artifact.type})</span> — {artifact.reason}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="limits-title">
        <h3 id="limits-title">Limits of this capsule</h3>
        {capsule.limitations.length === 0 ? (
          <p className="empty">None stated.</p>
        ) : (
          <ul className="plain">
            {capsule.limitations.map((limit) => (
              <li key={limit}>{limit}</li>
            ))}
          </ul>
        )}
      </section>
    </section>
  );
});
