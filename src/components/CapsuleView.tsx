import { forwardRef, type ReactNode } from "react";
import type { Checkpoint, CheckpointPayload, ResumeCapsule } from "../bindings/contracts";
import { supportProblem } from "../lib/capsuleEdit";
import { SourceList } from "./SourceList";

const CONFIDENCE: Record<string, string> = {
  low: "low confidence",
  medium: "medium confidence",
  high: "high confidence",
};

interface CapsuleViewProps {
  checkpoint: Checkpoint;
  /** Which capsule to show. Defaults to the model's draft. */
  capsule?: ResumeCapsule | null;
  title?: string;
  /** Replaces the default note under the title. */
  intro?: ReactNode;
  /** Keeps element IDs unique when two capsules are on screen. */
  idPrefix?: string;
  /** Heading level for the title; the sections below it are one level deeper. */
  headingLevel?: 2 | 3;
  /** Called only when the user explicitly clicks Copy next action. */
  onCopyInstruction?: (instruction: string) => void;
  /** Called only when the user explicitly clicks Open file on a relevant artifact. */
  onOpenArtifact?: (relativePath: string) => void;
}

/**
 * A capsule, read-only. Facts, hypotheses, failed attempts, and open questions
 * stay in separate sections and are labelled in words, so how sure the capsule
 * is can be read without relying on colour. Every source opens its evidence.
 */
export const CapsuleView = forwardRef<HTMLHeadingElement, CapsuleViewProps>(function CapsuleView(
  {
    checkpoint,
    capsule: shown,
    title = "Resume capsule (draft)",
    intro,
    idPrefix = "capsule",
    headingLevel = 2,
    onCopyInstruction,
    onOpenArtifact,
  },
  headingRef,
) {
  const capsule = shown === undefined ? (checkpoint.draft as ResumeCapsule | null) : shown;
  if (!capsule) return null;
  const entries = (checkpoint.payload as unknown as CheckpointPayload).entries;
  const id = (name: string) => `${idPrefix}-${name}`;
  const Title = `h${headingLevel}` as "h2" | "h3";
  const Section = `h${headingLevel + 1}` as "h3" | "h4";

  return (
    <section className="panel capsule" aria-labelledby={id("title")}>
      <Title id={id("title")} tabIndex={-1} ref={headingRef}>
        {title}
      </Title>
      {intro === undefined ? (
        <p className="hint">
          Written by {checkpoint.modelId ?? "the model"} from the evidence you approved
          {checkpoint.promptVersion ? ` (prompt ${checkpoint.promptVersion})` : ""}. It is a draft: review and
          approval come next, and nothing here is final.
        </p>
      ) : (
        intro
      )}

      <section aria-labelledby={id("next-title")} className="capsule-next">
        <div className="section-head">
          <Section id={id("next-title")}>Next action</Section>
          {onCopyInstruction ? (
            <button
              type="button"
              onClick={() => onCopyInstruction(capsule.next_action.instruction)}
            >
              Copy next action
            </button>
          ) : null}
        </div>
        <p className="preformatted">{capsule.next_action.instruction}</p>
        <p className="detail">
          <strong>Why:</strong> {capsule.next_action.justification}
        </p>
        <p className="detail">
          <strong>You will know it worked when:</strong> {capsule.next_action.success_check}
        </p>
        <SourceList ids={capsule.next_action.source_event_ids} entries={entries} />
      </section>

      <section aria-labelledby={id("summary-title")}>
        <Section id={id("summary-title")}>Where things stand</Section>
        <p className="preformatted">{capsule.checkpoint_summary}</p>
        <p className="hint">Goal: {capsule.goal}</p>
      </section>

      <section aria-labelledby={id("facts-title")}>
        <Section id={id("facts-title")}>Verified facts</Section>
        {capsule.verified_facts.length === 0 ? (
          <p className="empty">No claim in this capsule is backed by evidence strongly enough to call it a fact.</p>
        ) : (
          <ul className="plain">
            {capsule.verified_facts.map((fact) => {
              const problem = supportProblem(fact.source_event_ids, entries);
              return (
                <li key={fact.id}>
                  {problem ? (
                    <>
                      <span aria-hidden="true">⚠ </span>
                      <strong>Needs review: </strong>
                    </>
                  ) : (
                    <>
                      <span aria-hidden="true">✓ </span>
                      <span className="visually-hidden">Verified: </span>
                    </>
                  )}
                  {fact.claim}
                  {problem ? <span className="hint"> {problem}</span> : null}
                  <SourceList ids={fact.source_event_ids} entries={entries} />
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section aria-labelledby={id("hypotheses-title")}>
        <Section id={id("hypotheses-title")}>Hypotheses (not proven)</Section>
        {capsule.hypotheses.length === 0 ? (
          <p className="empty">None.</p>
        ) : (
          <ul className="plain">
            {capsule.hypotheses.map((hypothesis) => (
              <li key={hypothesis.id}>
                <span aria-hidden="true">? </span>
                <span className="visually-hidden">Hypothesis: </span>
                {hypothesis.claim}{" "}
                <span className="hint">({CONFIDENCE[hypothesis.confidence] ?? hypothesis.confidence})</span>
                <SourceList ids={hypothesis.source_event_ids} entries={entries} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby={id("attempts-title")}>
        <Section id={id("attempts-title")}>Already tried</Section>
        {capsule.failed_attempts.length === 0 ? (
          <p className="empty">Nothing recorded.</p>
        ) : (
          <ul className="plain">
            {capsule.failed_attempts.map((attempt, index) => (
              <li key={index}>
                <strong>{attempt.action}</strong> — {attempt.outcome} <em>Learned: {attempt.learning}</em>
                <SourceList ids={attempt.source_event_ids} entries={entries} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby={id("questions-title")}>
        <Section id={id("questions-title")}>Still unknown</Section>
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

      <section aria-labelledby={id("artifacts-title")}>
        <Section id={id("artifacts-title")}>Files that matter</Section>
        {capsule.relevant_artifacts.length === 0 ? (
          <p className="empty">None named.</p>
        ) : (
          <ul className="plain">
            {capsule.relevant_artifacts.map((artifact) => (
              <li key={artifact.relative_path}>
                <code>{artifact.relative_path}</code> <span className="hint">({artifact.type})</span> — {artifact.reason}
                {onOpenArtifact ? (
                  <>
                    {" "}
                    <button
                      type="button"
                      onClick={() => onOpenArtifact(artifact.relative_path)}
                      aria-label={`Open file ${artifact.relative_path}`}
                    >
                      Open file
                    </button>
                  </>
                ) : null}
                <SourceList ids={artifact.source_event_ids} entries={entries} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby={id("limits-title")}>
        <Section id={id("limits-title")}>Limits of this capsule</Section>
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
