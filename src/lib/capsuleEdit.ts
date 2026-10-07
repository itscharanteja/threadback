import type {
  CheckpointEntry,
  Confidence,
  Hypothesis,
  NoteLabel,
  ResumeCapsule,
  SourcedClaim,
} from "../bindings/contracts";

/** A deep copy, so editing never touches the model's draft. */
export const cloneCapsule = (capsule: ResumeCapsule): ResumeCapsule => JSON.parse(JSON.stringify(capsule));

export const sameCapsule = (a: ResumeCapsule, b: ResumeCapsule): boolean => JSON.stringify(a) === JSON.stringify(b);

/** Notes with these labels are the developer's own uncertainty, so they cannot prove a fact. */
const UNPROVEN: NoteLabel[] = ["hypothesis", "question", "next_step"];

/**
 * Why a verified fact would not be accepted with these sources, or null when it
 * would. This mirrors the server's rules to explain a problem early; the server
 * check on approval remains the authority.
 */
export function supportProblem(ids: string[], entries: CheckpointEntry[]): string | null {
  if (ids.length === 0) return "It cites no evidence.";
  const byId = new Map(entries.map((entry) => [entry.eventId, entry]));
  if (ids.some((id) => !byId.has(id))) return "It cites an event that is not in this checkpoint.";
  const canSupport = ids.some((id) => {
    const entry = byId.get(id)!;
    if (entry.kind !== "note.added") return true;
    const label = (entry.content as { label?: NoteLabel }).label;
    return !label || !UNPROVEN.includes(label);
  });
  return canSupport ? null : "It rests only on notes you labelled hypothesis, question, or next step.";
}

export interface FactProblem {
  index: number;
  message: string;
}

/** Verified facts that would not be accepted, by position. */
export function factProblems(capsule: ResumeCapsule, entries: CheckpointEntry[]): FactProblem[] {
  const problems: FactProblem[] = [];
  capsule.verified_facts.forEach((fact, index) => {
    const message = supportProblem(fact.source_event_ids, entries);
    if (message) problems.push({ index, message });
  });
  return problems;
}

const nextId = (prefix: string, used: string[]): string => {
  let n = used.length + 1;
  while (used.includes(`${prefix}_${n}`)) n += 1;
  return `${prefix}_${n}`;
};

export const removeAt = <T>(list: T[], index: number): T[] => list.filter((_, position) => position !== index);

export const replaceAt = <T>(list: T[], index: number, change: Partial<T>): T[] =>
  list.map((item, position) => (position === index ? { ...item, ...change } : item));

/** Adds the event to the sources, or takes it away when it is already there. */
export const toggleSource = (ids: string[], id: string): string[] =>
  ids.includes(id) ? ids.filter((existing) => existing !== id) : [...ids, id];

/** Moves a verified fact to the hypotheses, keeping its text and sources. */
export function factToHypothesis(capsule: ResumeCapsule, index: number, confidence: Confidence): ResumeCapsule {
  const fact = capsule.verified_facts[index];
  if (!fact) return capsule;
  const hypothesis: Hypothesis = {
    id: nextId("hyp", capsule.hypotheses.map((h) => h.id)),
    claim: fact.claim,
    confidence,
    source_event_ids: [...fact.source_event_ids],
  };
  return {
    ...capsule,
    verified_facts: removeAt(capsule.verified_facts, index),
    hypotheses: [...capsule.hypotheses, hypothesis],
  };
}

/** Moves a hypothesis to the verified facts. The caller must check it has support first. */
export function hypothesisToFact(capsule: ResumeCapsule, index: number): ResumeCapsule {
  const hypothesis = capsule.hypotheses[index];
  if (!hypothesis) return capsule;
  const fact: SourcedClaim = {
    id: nextId("fact", capsule.verified_facts.map((f) => f.id)),
    claim: hypothesis.claim,
    source_event_ids: [...hypothesis.source_event_ids],
  };
  return {
    ...capsule,
    hypotheses: removeAt(capsule.hypotheses, index),
    verified_facts: [...capsule.verified_facts, fact],
  };
}

/** Where an error's JSON Pointer (such as `/verified_facts/0/claim`) belongs on screen. */
export const fieldId = (path: string): string => `capsule-field${path.replace(/[^A-Za-z0-9]+/g, "-")}`;
