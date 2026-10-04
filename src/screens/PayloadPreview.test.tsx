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
    <ThreadScreen thread={thread} project={project} refreshMs={60_000} onChanged={async () => setThread({ ...backend.thread })} />
  );
}

const button = (name: RegExp | string) => screen.getByRole("button", { name });

/** A thread with notes, saved as a draft, with the preview open. */
async function openPreview(notes: { text: string; label: "fact" | "hypothesis" }[] = [
  { text: "The focused test made two provider calls.", label: "fact" },
  { text: "A second worker bypasses the lock.", label: "hypothesis" },
]) {
  backend = new FakeBackend("ACTIVE");
  bridge.invoke = backend.invoke;
  for (const note of notes) await backend.invoke("events_add_note", { threadId: "thread-1", note });
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(button("Save My Place"));
  await screen.findByRole("heading", { name: "Checkpoint draft" });
  await user.click(button("Review what will be sent…"));
  await screen.findByRole("heading", { name: "Review what will be sent" });
  await screen.findByRole("list", { name: "Entries in the payload" });
  return user;
}

const generateButton = () => screen.getByRole("button", { name: /Generate capsule|Requesting/ });
const entryBox = (name: RegExp) => screen.getByRole("checkbox", { name });

describe("reviewing the payload", () => {
  it("shows exactly what would be sent and sends nothing by looking", async () => {
    await openPreview();
    expect(screen.getByRole("heading", { name: "Review what will be sent" })).toHaveFocus();
    expect(screen.getByText(/Nothing has been sent\./)).toBeInTheDocument();
    expect(screen.getByText(/3 entries will be sent/)).toBeInTheDocument();

    const entries = screen.getByRole("list", { name: "Entries in the payload" });
    expect(within(entries).getByText("A second worker bypasses the lock.")).toBeInTheDocument();
    expect(within(entries).getAllByText("Will be sent", { exact: false }).length).toBe(3);
    expect(screen.getAllByRole("checkbox")).toHaveLength(3);
    screen.getAllByRole("checkbox").forEach((box) => expect(box).toBeChecked());

    expect(backend.callsTo("checkpoints_preview")[0].args).toEqual({ checkpointId: "checkpoint-1", excludedEventIds: [] });
    expect(backend.callsTo("checkpoints_generate")).toHaveLength(0);
  });

  it("shows the complete document and each entry's exact content", async () => {
    const user = await openPreview();
    await user.click(screen.getByText(/Show the complete payload as one document/));
    const document = screen.getAllByText(/"goal"/).find((node) => node.tagName === "PRE")!;
    expect(document).toHaveTextContent("Find why retries create a second provider capture");
    expect(screen.getAllByText("Exact content sent").length).toBe(3);
  });

  it("excluding an entry removes it from the payload and asks the server again", async () => {
    const user = await openPreview();
    await user.click(entryBox(/Hypothesis/));
    await screen.findByText(/2 entries will be sent/);
    expect(entryBox(/Hypothesis/)).not.toBeChecked();
    expect(screen.getByText(/1 excluded/)).toBeInTheDocument();
    const last = backend.callsTo("checkpoints_preview").at(-1)!.args;
    expect(last.excludedEventIds).toHaveLength(1);
    expect(screen.getByText("Excluded by you", { exact: false })).toBeInTheDocument();

    await user.click(entryBox(/Hypothesis/));
    await screen.findByText(/3 entries will be sent/);
  });

  it("keeps a kind of evidence that is never sent on this computer", async () => {
    backend = new FakeBackend("ACTIVE");
    bridge.invoke = backend.invoke;
    await backend.invoke("events_add_note", { threadId: "thread-1", note: { text: "fine", label: "fact" } });
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(button("Save My Place"));
    await screen.findByRole("heading", { name: "Checkpoint draft" });
    backend.addCheckpointEntry({ eventId: "keys", kind: "terminal.keystrokes", content: { text: "hello" } });
    await user.click(button("Review what will be sent…"));
    await screen.findByRole("list", { name: "Entries in the payload" });

    const boxes = screen.getAllByRole("checkbox");
    const local = boxes.find((box) => box.hasAttribute("disabled"))!;
    expect(local).not.toBeChecked();
    expect(screen.getByText(/Stays on this computer/)).toBeInTheDocument();
    expect(screen.getByText(/1 stay on this computer/)).toBeInTheDocument();
  });
});

describe("blocked entries", () => {
  const withCredential = [
    { text: "The test fails.", label: "fact" as const },
    { text: "The culprit is ACME-123456", label: "hypothesis" as const },
  ];

  it("blocks generation until the entry is excluded, and says why", async () => {
    const user = await openPreview(withCredential);
    expect(screen.getByRole("alert")).toHaveTextContent(/1 entry contains a credential and cannot be sent/);
    expect(generateButton()).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByText(/Generate is unavailable: 1 entry contains a credential/)).toBeInTheDocument();
    expect(generateButton()).toHaveAccessibleDescription(/Generate is unavailable/);

    // The preview never shows the credential, only its replacement. (The local
    // evidence timeline further down still holds the stored text; it is not sent.)
    const panel = screen.getByRole("heading", { name: "Review what will be sent" }).closest("section")!;
    expect(within(panel).queryByText(/ACME-123456/)).toBeNull();
    expect(panel.textContent).not.toContain("ACME-123456");
    expect(within(panel).getAllByText(/\[REDACTED:custom\]/).length).toBeGreaterThan(0);

    // Pressing the disabled button does nothing.
    await user.click(generateButton());
    expect(backend.callsTo("checkpoints_generate")).toHaveLength(0);

    await user.click(entryBox(/Hypothesis/));
    await waitFor(() => expect(generateButton()).toHaveAttribute("aria-disabled", "false"));
    expect(screen.queryByText(/Generate is unavailable/)).toBeNull();
  });

  it("stays unavailable when nothing is selected", async () => {
    const user = await openPreview([{ text: "only note", label: "fact" }]);
    for (const box of screen.getAllByRole("checkbox")) await user.click(box);
    await screen.findByText(/Generate is unavailable: nothing is selected/);
    expect(generateButton()).toHaveAttribute("aria-disabled", "true");
    await user.click(generateButton());
    expect(backend.callsTo("checkpoints_generate")).toHaveLength(0);
  });
});

describe("generating", () => {
  it("asks for generation only when pressed, with the reviewed selection and hash", async () => {
    const user = await openPreview();
    await user.click(entryBox(/Hypothesis/));
    await screen.findByText(/2 entries will be sent/);
    expect(backend.callsTo("checkpoints_generate")).toHaveLength(0);

    await user.click(generateButton());
    await waitFor(() => expect(backend.callsTo("checkpoints_generate")).toHaveLength(1));
    const args = backend.callsTo("checkpoints_generate")[0].args;
    expect(args.excludedEventIds).toHaveLength(1);
    const reviewed = (await backend.invoke("checkpoints_preview", {
      checkpointId: "checkpoint-1",
      excludedEventIds: args.excludedEventIds,
    })) as { payloadHash: string };
    expect(args.reviewedHash).toBe(reviewed.payloadHash);
    // No model connection yet: the message says nothing was sent, and the screen stays put.
    expect(await screen.findByText(/Nothing was sent/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Review what will be sent" })).toBeInTheDocument();
  });

  it("reloads the payload when it changed since it was reviewed", async () => {
    const user = await openPreview();
    const before = backend.callsTo("checkpoints_preview").length;
    backend.failNext("checkpoints_generate", "What would be sent has changed since you reviewed it. Review it again.", null, "PAYLOAD_CHANGED");
    await user.click(generateButton());
    expect(await screen.findByText(/has changed since you reviewed it/)).toBeInTheDocument();
    await waitFor(() => expect(backend.callsTo("checkpoints_preview").length).toBeGreaterThan(before));
  });
});

describe("leaving the review", () => {
  it("cancel returns to the draft and sends nothing", async () => {
    const user = await openPreview();
    await user.click(entryBox(/Hypothesis/));
    await user.click(button("Cancel"));
    expect(await screen.findByRole("heading", { name: "Checkpoint draft" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Review what will be sent" })).toBeNull();
    expect(backend.callsTo("checkpoints_generate")).toHaveLength(0);
    expect(backend.callsTo("checkpoints_discard")).toHaveLength(0);
  });

  it("discarding needs a confirmation, then returns the thread to recording", async () => {
    const user = await openPreview();
    await user.click(button("Discard draft and keep working…"));
    expect(backend.callsTo("checkpoints_discard")).toHaveLength(0);
    expect(screen.getByText(/Recording resumes and nothing is lost/)).toBeInTheDocument();

    await user.click(button("Keep draft"));
    expect(backend.callsTo("checkpoints_discard")).toHaveLength(0);

    await user.click(button("Discard draft and keep working…"));
    await user.click(button("Discard draft"));
    await waitFor(() => expect(backend.callsTo("checkpoints_discard")).toHaveLength(1));
    expect(await screen.findByText("Active — recording evidence")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Review what will be sent" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Checkpoint draft" })).toBeNull();
    expect(screen.getByLabelText(/^Note/)).toBeEnabled();
    expect(backend.callsTo("checkpoints_generate")).toHaveLength(0);
  });

  it("discarding works from the draft view too", async () => {
    backend = new FakeBackend("ACTIVE");
    bridge.invoke = backend.invoke;
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(button("Save My Place"));
    await screen.findByRole("heading", { name: "Checkpoint draft" });
    await user.click(button("Discard draft and keep working…"));
    await user.click(button("Discard draft"));
    expect(await screen.findByText("Active — recording evidence")).toBeInTheDocument();
  });

  it("shows a refusal to discard", async () => {
    const user = await openPreview();
    backend.failNext("checkpoints_discard", "Another thread is already recording. Pause or end it first.");
    await user.click(button("Discard draft and keep working…"));
    await user.click(button("Discard draft"));
    expect(await screen.findByText(/Another thread is already recording/)).toBeInTheDocument();
    expect(screen.getByText("Checkpoint draft — not recording")).toBeInTheDocument();
  });
});
