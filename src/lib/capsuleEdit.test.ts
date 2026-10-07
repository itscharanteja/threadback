import { describe, expect, it } from "vitest";
import type { CheckpointEntry, ResumeCapsule } from "../bindings/contracts";
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
} from "./capsuleEdit";

const baseCapsule: ResumeCapsule = {
  schema_version: "1.0",
  goal: "Test goal",
  checkpoint_summary: "Summary",
  verified_facts: [
    { id: "fact_1", claim: "Fact 1", source_event_ids: ["evt-1"] },
  ],
  hypotheses: [
    { id: "hyp_1", claim: "Hypothesis 1", confidence: "medium", source_event_ids: ["evt-2"] },
  ],
  failed_attempts: [],
  unresolved_questions: [],
  relevant_artifacts: [],
  next_action: {
    instruction: "Do something",
    justification: "Because",
    success_check: "Works",
    source_event_ids: [],
  },
  limitations: [],
};

const entries: CheckpointEntry[] = [
  {
    eventId: "evt-1",
    timestamp: "2026-10-07T00:00:00Z",
    kind: "note.added",
    content: { text: "Fact note", label: "fact" },
  },
  {
    eventId: "evt-2",
    timestamp: "2026-10-07T00:01:00Z",
    kind: "note.added",
    content: { text: "Hyp note", label: "hypothesis" },
  },
  {
    eventId: "evt-3",
    timestamp: "2026-10-07T00:02:00Z",
    kind: "command.finished",
    content: { exitCode: 1 },
  },
];

describe("capsuleEdit helpers", () => {
  it("clones and compares capsules deeply", () => {
    const copy = cloneCapsule(baseCapsule);
    expect(sameCapsule(baseCapsule, copy)).toBe(true);
    copy.goal = "Changed";
    expect(sameCapsule(baseCapsule, copy)).toBe(false);
    expect(baseCapsule.goal).toBe("Test goal");
  });

  it("checks support problems for verified facts", () => {
    expect(supportProblem([], entries)).toBe("It cites no evidence.");
    expect(supportProblem(["evt-99"], entries)).toBe("It cites an event that is not in this checkpoint.");
    expect(supportProblem(["evt-2"], entries)).toBe(
      "It rests only on notes you labelled hypothesis, question, or next step.",
    );
    expect(supportProblem(["evt-1"], entries)).toBeNull();
    expect(supportProblem(["evt-2", "evt-3"], entries)).toBeNull();
  });

  it("lists fact problems across a capsule", () => {
    const copy = cloneCapsule(baseCapsule);
    copy.verified_facts.push({ id: "fact_2", claim: "Unsupported", source_event_ids: ["evt-2"] });
    expect(factProblems(copy, entries)).toEqual([
      { index: 1, message: "It rests only on notes you labelled hypothesis, question, or next step." },
    ]);
  });

  it("relabels between facts and hypotheses with non-colliding IDs", () => {
    const demoted = factToHypothesis(baseCapsule, 0, "low");
    expect(demoted.verified_facts).toHaveLength(0);
    expect(demoted.hypotheses).toHaveLength(2);
    expect(demoted.hypotheses[1]).toEqual({
      id: "hyp_2",
      claim: "Fact 1",
      confidence: "low",
      source_event_ids: ["evt-1"],
    });

    const promoted = hypothesisToFact(demoted, 1);
    expect(promoted.verified_facts).toHaveLength(1);
    expect(promoted.verified_facts[0].id).toBe("fact_1");
    expect(promoted.hypotheses).toHaveLength(1);
  });

  it("removes, replaces, toggles sources, and builds field IDs", () => {
    expect(removeAt(["a", "b", "c"], 1)).toEqual(["a", "c"]);
    expect(replaceAt([{ x: 1, y: 2 }], 0, { y: 3 })).toEqual([{ x: 1, y: 3 }]);
    expect(toggleSource(["evt-1"], "evt-2")).toEqual(["evt-1", "evt-2"]);
    expect(toggleSource(["evt-1", "evt-2"], "evt-1")).toEqual(["evt-2"]);
    expect(fieldId("/verified_facts/0/claim")).toBe("capsule-field-verified-facts-0-claim");
  });
});
