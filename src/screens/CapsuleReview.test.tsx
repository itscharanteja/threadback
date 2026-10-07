import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { Project } from "../bindings/contracts";
import { FakeBackend } from "../test/fakeBackend";
import { ThreadScreen } from "./ThreadScreen";

const bridge = vi.hoisted(() => ({
  invoke: null as null | ((command: string, args?: unknown) => Promise<unknown>),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => bridge.invoke!(command, args),
}));

const project: Project = {
  id: "project-1",
  displayName: "Payments",
  canonicalRoot: "/work/payments",
  createdAt: new Date().toISOString(),
  updatedAt: new Date().toISOString(),
};

let backend: FakeBackend;

function Harness() {
  const [thread, setThread] = useState(backend.thread);
  return (
    <ThreadScreen
      thread={thread}
      project={project}
      refreshMs={60_000}
      onChanged={async () => setThread({ ...backend.thread })}
    />
  );
}

const button = (name: RegExp | string) => screen.getByRole("button", { name });
const approveButton = () => screen.getByRole("button", { name: /Approve capsule|Approving/ });

/** A thread with a fact note (evt-2) and a hypothesis note (evt-3), with a generated capsule on screen. */
async function openCapsule(capsule?: unknown) {
  backend = new FakeBackend("ACTIVE");
  bridge.invoke = backend.invoke;
  if (capsule) backend.capsule = capsule;
  backend.config.hasApiKey = true;
  await backend.invoke("events_add_note", { threadId: "thread-1", note: { text: "The focused test made two provider calls.", label: "fact" } });
  await backend.invoke("events_add_note", { threadId: "thread-1", note: { text: "A second worker bypasses the lock.", label: "hypothesis" } });
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(button("Save My Place"));
  await user.click(await screen.findByRole("button", { name: "Review what will be sent…" }));
  await screen.findByRole("list", { name: "Entries in the payload" });
  await user.click(screen.getByRole("button", { name: /Generate capsule/ }));
  await screen.findByRole("heading", { name: "Resume capsule (draft)" });
  return user;
}

/** The fake's default capsule, for a test to adjust. */
const baseCapsule = () => {
  const probe = new FakeBackend("ACTIVE");
  return JSON.parse(JSON.stringify(probe.capsule));
};
const factItem = () => screen.getByLabelText("Claim for fact 1").closest("li")!;

describe("reviewing a generated capsule", () => {
  it("lets every source be opened to its recorded evidence", async () => {
    const user = await openCapsule();
    const item = factItem();
    expect(within(item).queryByText("The focused test made two provider calls.")).toBeNull();
    await user.click(within(item).getByRole("button", { name: "Source 1: Fact" }));
    expect(within(item).getByText("The focused test made two provider calls.")).toBeInTheDocument();
    await user.click(within(item).getByRole("button", { name: "Source 1: Fact" }));
    expect(within(item).queryByText("The focused test made two provider calls.")).toBeNull();
  });

  it("shows an unresolvable source as such and blocks approval until the fact is fixed", async () => {
    const capsule = baseCapsule();
    capsule.verified_facts[0].source_event_ids = ["evt-99"];
    const user = await openCapsule(capsule);

    expect(within(factItem()).getByText("Needs review.")).toBeInTheDocument();
    expect(within(factItem()).getByText(/not in this checkpoint/)).toBeInTheDocument();
    await user.click(within(factItem()).getByRole("button", { name: /Source 1: not found/ }));
    expect(within(factItem()).getByRole("alert")).toHaveTextContent(/cannot back a verified fact/);

    expect(approveButton()).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByText(/Approval is unavailable while 1 fact is marked Needs review/)).toBeInTheDocument();
    await user.click(approveButton());
    expect(backend.callsTo("checkpoints_approve")).toHaveLength(0);

    await user.click(button("Remove fact 1"));
    expect(approveButton()).toHaveAttribute("aria-disabled", "false");
  });

  it("does not accept a fact that rests only on the developer's own hypothesis note", async () => {
    const capsule = baseCapsule();
    capsule.verified_facts[0].source_event_ids = ["evt-3"];
    await openCapsule(capsule);
    expect(within(factItem()).getByText(/rests only on notes you labelled hypothesis/)).toBeInTheDocument();
    expect(approveButton()).toHaveAttribute("aria-disabled", "true");
  });

  it("relabels a fact as a hypothesis, approves it, and keeps the model's draft unchanged", async () => {
    const user = await openCapsule();
    await user.click(button("Make fact 1 a hypothesis"));
    expect(screen.getByText(/No claim in this capsule is a verified fact\./)).toBeInTheDocument();
    expect(screen.getByLabelText("Claim for hypothesis 2")).toHaveValue("The focused retry test made two provider calls.");
    expect(screen.getByLabelText("Confidence for hypothesis 2")).toHaveDisplayValue("Low confidence");

    await user.click(approveButton());
    const heading = await screen.findByRole("heading", { name: "Approved resume capsule" });
    await waitFor(() => expect(heading).toHaveFocus());
    expect(backend.thread.state).toBe("READY");

    expect(screen.getByRole("heading", { name: "Your corrections" })).toBeInTheDocument();
    const list = screen.getByRole("list", { name: "Recorded corrections" });
    expect(within(list).getByText("/verified_facts")).toBeInTheDocument();
    expect(within(list).getByText("/hypotheses")).toBeInTheDocument();

    // The model's draft is separate and untouched.
    const stored = backend.checkpoints[0];
    expect((stored.draft as any).verified_facts).toHaveLength(1);
    expect((stored.approved as any).verified_facts).toHaveLength(0);
    await user.click(button("Show the original model draft"));
    const original = screen.getByRole("heading", { name: "Original model draft" }).closest("section")!;
    expect(within(original).getByText(/The focused retry test made two provider calls\./)).toBeInTheDocument();
    // An approved capsule can no longer be discarded.
    expect(screen.queryByRole("button", { name: /Discard draft and keep working/ })).toBeNull();
  });

  it("will not make a hypothesis a verified fact until it has support", async () => {
    const user = await openCapsule();
    const promote = button("Make hypothesis 1 a verified fact");
    expect(promote).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByText(/Cannot be a verified fact yet: It cites no evidence\./)).toBeInTheDocument();
    await user.click(promote);
    expect(screen.getByLabelText("Claim for fact 1")).toBeInTheDocument();
    expect(screen.queryByLabelText("Claim for fact 2")).toBeNull();

    await user.click(button("Edit sources for hypothesis 1"));
    const picker = screen.getByRole("group", { name: "Sources for hypothesis 1" });
    await user.click(within(picker).getByRole("checkbox", { name: /Fact: The focused test made two provider calls\./ }));
    expect(button("Make hypothesis 1 a verified fact")).toHaveAttribute("aria-disabled", "false");

    await user.click(button("Make hypothesis 1 a verified fact"));
    expect(screen.getByLabelText("Claim for fact 2")).toHaveValue("A second worker bypasses the distributed lock.");
    expect(screen.queryByLabelText("Claim for hypothesis 1")).toBeNull();
  });

  it("sends the user's edits as the final capsule and records no corrections for an untouched draft", async () => {
    const user = await openCapsule();
    expect(screen.getByText(/No changes yet\. Approving now records no corrections\./)).toBeInTheDocument();
    await user.click(approveButton());
    await screen.findByRole("heading", { name: "Approved resume capsule" });
    expect(screen.getByText(/None\. You approved the model's draft as it was\./)).toBeInTheDocument();
  });

  it("edits text, and reports unsaved edits", async () => {
    const user = await openCapsule();
    const instruction = screen.getByLabelText("Next action instruction");
    await user.clear(instruction);
    await user.type(instruction, "Check the lock backend config.");
    expect(screen.getByText(/You have edits that are kept only in this window/)).toBeInTheDocument();

    await user.click(approveButton());
    await screen.findByRole("heading", { name: "Approved resume capsule" });
    const sent = backend.callsTo("checkpoints_approve")[0].args;
    expect(sent.checkpointId).toBe("checkpoint-1");
    expect(sent.capsule.next_action.instruction).toBe("Check the lock backend config.");
    expect((backend.checkpoints[0].draft as any).next_action.instruction).toBe("Log the lock owner in both workers and rerun the focused test.");
    expect(screen.getByText(/1 change recorded against the model's draft\./)).toBeInTheDocument();
  });

  it("puts focus on the field the server rejected and keeps the edits", async () => {
    const user = await openCapsule();
    const instruction = screen.getByLabelText("Next action instruction");
    await user.clear(instruction);
    await user.type(instruction, "Temporary");
    backend.failNext("checkpoints_approve", "This capsule cannot be approved yet. The next action needs an instruction.", "/next_action/instruction", "INVALID_CAPSULE");
    await user.click(approveButton());

    expect(await screen.findByRole("alert")).toHaveTextContent(/next action needs an instruction/);
    await waitFor(() => expect(instruction).toHaveFocus());
    expect(instruction).toHaveAttribute("aria-invalid", "true");
    expect(instruction).toHaveValue("Temporary");
    expect(screen.getByRole("heading", { name: "Resume capsule (draft)" })).toBeInTheDocument();
  });

  it("removes items, adds questions, and discards the user's edits", async () => {
    const user = await openCapsule();
    await user.click(button("Remove open question 1"));
    expect(screen.getByText("No open questions.")).toBeInTheDocument();
    await user.type(screen.getByPlaceholderText("Add an open question"), "Who owns the lock?");
    await user.click(within(screen.getByPlaceholderText("Add an open question").closest("div")!).getByRole("button", { name: "Add" }));
    expect(screen.getByLabelText("Open question 1")).toHaveValue("Who owns the lock?");

    await user.click(button("Discard my edits"));
    expect(screen.getByLabelText("Open question 1")).toHaveValue("Do both workers share the same lock backend?");
    expect(screen.getByText(/No changes yet/)).toBeInTheDocument();
    expect(button("Discard my edits")).toBeDisabled();
  });

  it("shows the original model draft read-only next to the working copy", async () => {
    const user = await openCapsule();
    await user.click(button("Make fact 1 a hypothesis"));
    await user.click(button("Show the original model draft"));
    const original = screen.getByRole("heading", { name: "Original model draft" }).closest("section")!;
    expect(within(original).getByRole("heading", { name: "Verified facts" })).toBeInTheDocument();
    expect(within(original).getByText(/The focused retry test made two provider calls\./)).toBeInTheDocument();
    expect(within(original).queryByRole("textbox")).toBeNull();
    await user.click(button("Hide the original model draft"));
    expect(screen.queryByRole("heading", { name: "Original model draft" })).toBeNull();
  });

  it("regenerates by discarding the draft, saving the place again, and returning to the payload review", async () => {
    const user = await openCapsule();
    await user.clear(screen.getByLabelText("Next action instruction"));
    await user.click(button("Regenerate…"));
    expect(screen.getByText(/throws away this draft and your edits/)).toBeInTheDocument();
    await user.click(button("Keep this draft"));
    expect(backend.callsTo("checkpoints_discard")).toHaveLength(0);

    await user.click(button("Regenerate…"));
    await user.click(button("Discard and regenerate"));
    const heading = await screen.findByRole("heading", { name: "Review what will be sent" });
    await waitFor(() => expect(heading).toHaveFocus());
    expect(backend.callsTo("checkpoints_discard")).toHaveLength(1);
    expect(backend.callsTo("checkpoints_save_place")).toHaveLength(2);
    expect(backend.checkpoints[0].status).toBe("draft");
    expect(backend.checkpoints[1].status).toBe("discarded");
    expect(screen.queryByRole("heading", { name: "Resume capsule (draft)" })).toBeNull();
  });

  it("is operable by keyboard alone", async () => {
    const user = await openCapsule();
    screen.getByLabelText("Claim for fact 1").focus();
    await user.keyboard("{Tab}");
    // Next stop is the first source button of the fact.
    expect(document.activeElement).toBe(within(factItem()).getByRole("button", { name: "Source 1: Fact" }));
    await user.keyboard("{Enter}");
    expect(within(factItem()).getByText("The focused test made two provider calls.")).toBeInTheDocument();
  });
});
