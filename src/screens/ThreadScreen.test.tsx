import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, ThreadState } from "../bindings/contracts";
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

function Harness({ refreshMs = 60_000 }: { refreshMs?: number }) {
  const [thread, setThread] = useState(backend.thread);
  return (
    <ThreadScreen
      thread={thread}
      project={project}
      refreshMs={refreshMs}
      onChanged={async () => setThread({ ...backend.thread })}
    />
  );
}

function open(state: ThreadState = "ACTIVE", refreshMs?: number) {
  backend = new FakeBackend(state);
  bridge.invoke = backend.invoke;
  const user = userEvent.setup();
  render(<Harness refreshMs={refreshMs} />);
  return user;
}

const button = (name: RegExp | string) => screen.getByRole("button", { name });

describe("notes", () => {
  it("adds a note with the chosen label and the text exactly as typed", async () => {
    const user = open();
    await user.click(screen.getByRole("radio", { name: /Hypothesis/ }));
    await user.type(screen.getByLabelText(/^Note/), "  A second worker bypasses the lock");
    await user.click(button("Add note"));

    await waitFor(() => expect(backend.callsTo("events_add_note")).toHaveLength(1));
    expect(backend.callsTo("events_add_note")[0].args).toEqual({
      threadId: "thread-1",
      note: { text: "  A second worker bypasses the lock", label: "hypothesis" },
    });
    const timeline = await screen.findByRole("list", { name: /Evidence timeline/ });
    expect(within(timeline).getByText("Hypothesis")).toBeInTheDocument();
    expect(within(timeline).getByText(/A second worker bypasses the lock/)).toBeInTheDocument();
    expect(screen.getByLabelText(/^Note/)).toHaveValue("");
  });

  it("submits with Ctrl+Enter from the note field", async () => {
    const user = open();
    await user.click(screen.getByRole("radio", { name: /Question/ }));
    await user.type(screen.getByLabelText(/^Note/), "Do both workers share a lock?{Control>}{Enter}{/Control}");
    await waitFor(() => expect(backend.callsTo("events_add_note")).toHaveLength(1));
  });

  it("starts with no label chosen and asks for one", async () => {
    const user = open();
    for (const radio of screen.getAllByRole("radio")) expect(radio).not.toBeChecked();

    await user.click(button("Add note"));
    expect(screen.getByText("Choose what kind of note this is.")).toBeInTheDocument();
    expect(screen.getAllByRole("radio")[0]).toHaveFocus();

    await user.click(screen.getByRole("radio", { name: /Fact/ }));
    await user.click(button("Add note"));
    expect(screen.getByText("Write the note first.")).toBeInTheDocument();
    expect(screen.getByLabelText(/^Note/)).toHaveFocus();
    expect(backend.callsTo("events_add_note")).toHaveLength(0);
  });

  it("shows a refusal from the backend and keeps what was typed", async () => {
    const user = open();
    await user.click(screen.getByRole("radio", { name: /Fact/ }));
    await user.type(screen.getByLabelText(/^Note/), "keep me");
    backend.failNext("events_add_note", "This thread is not active, so it is not recording evidence.");
    await user.click(button("Add note"));
    expect(await screen.findByRole("alert")).toHaveTextContent("not recording evidence");
    expect(screen.getByLabelText(/^Note/)).toHaveValue("keep me");
  });

  it("cannot be used while the thread is paused", () => {
    open("PAUSED");
    expect(screen.getByLabelText(/^Note/)).toBeDisabled();
    expect(button("Add note")).toBeDisabled();
    expect(screen.getByText(/Notes are recorded only while the thread is active/)).toBeInTheDocument();
    for (const radio of screen.getAllByRole("radio")) expect(radio).toBeDisabled();
  });
});

describe("thread controls", () => {
  it("offers pause, save, snapshot, and end while recording, and no resume", () => {
    open();
    expect(screen.getByText("Active — recording evidence")).toBeInTheDocument();
    for (const name of ["Save My Place", "Pause", "Take Git snapshot", "End thread…"]) {
      expect(button(name)).toBeEnabled();
    }
    expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();
  });

  it("pauses and then offers resume", async () => {
    const user = open();
    await user.click(button("Pause"));
    expect(await screen.findByText("Paused — not recording")).toBeInTheDocument();
    expect(button("Resume")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Save My Place" })).toBeNull();
    expect(screen.getByLabelText(/^Note/)).toBeDisabled();

    await user.click(button("Resume"));
    expect(await screen.findByText("Active — recording evidence")).toBeInTheDocument();
    expect(screen.getByLabelText(/^Note/)).toBeEnabled();
  });

  it("records a Git snapshot on request", async () => {
    const user = open();
    await user.click(button("Take Git snapshot"));
    const timeline = await screen.findByRole("list", { name: /Evidence timeline/ });
    expect(await within(timeline).findByText(/Branch main · HEAD 1234567 · working tree clean/)).toBeInTheDocument();
  });

  it("shows a refusal and re-enables the controls", async () => {
    const user = open();
    backend.failNext("threads_pause", "The thread changed while this action was in progress.");
    await user.click(button("Pause"));
    expect(await screen.findByRole("alert")).toHaveTextContent("changed while this action");
    expect(button("Pause")).toBeEnabled();
  });
});

describe("Save My Place", () => {
  it("saves a local draft, shows it, and stops recording", async () => {
    const user = open();
    await user.click(screen.getByRole("radio", { name: /Fact/ }));
    await user.type(screen.getByLabelText(/^Note/), "The focused test made two provider calls");
    await user.click(button("Add note"));
    await screen.findByText(/Note added\./, { selector: ".hint" });

    await user.click(button("Save My Place"));

    const heading = await screen.findByRole("heading", { name: "Checkpoint draft" });
    expect(heading).toHaveFocus();
    expect(screen.getByText("Checkpoint draft — not recording")).toBeInTheDocument();
    expect(screen.getByText(/Nothing has been sent anywhere/)).toBeInTheDocument();
    expect(screen.getByText(/Not taken — The folder is not inside a Git repository/)).toBeInTheDocument();
    const draft = screen.getByRole("list", { name: /Checkpoint evidence/ });
    expect(within(draft).getByText(/two provider calls/)).toBeInTheDocument();

    // Recording has stopped, so evidence cannot be added.
    expect(screen.queryByRole("button", { name: "Save My Place" })).toBeNull();
    expect(screen.getByLabelText(/^Note/)).toBeDisabled();
  });

  it("announces the result to assistive technology", async () => {
    const user = open();
    await user.click(button("Save My Place"));
    await screen.findByRole("heading", { name: "Checkpoint draft" });
    expect(screen.getAllByRole("status").some((node) => node.textContent?.includes("Checkpoint saved"))).toBe(true);
  });
});

describe("ending a thread", () => {
  it("asks for confirmation before ending", async () => {
    const user = open();
    await user.click(button("End thread…"));
    const heading = screen.getByRole("heading", { name: "End this thread?" });
    expect(heading).toHaveFocus();
    expect(backend.callsTo("threads_end")).toHaveLength(0);

    await user.click(button("Keep working"));
    expect(screen.queryByRole("heading", { name: "End this thread?" })).toBeNull();
    expect(backend.callsTo("threads_end")).toHaveLength(0);
    expect(button("Save My Place")).toBeInTheDocument();
  });

  it("saves a checkpoint when confirmed", async () => {
    const user = open();
    await user.click(button("End thread…"));
    await user.click(button("End thread and save checkpoint"));
    expect(await screen.findByRole("heading", { name: "Checkpoint draft" })).toHaveFocus();
    expect(backend.callsTo("threads_end")).toHaveLength(1);
    expect(screen.getByText("Checkpoint draft — not recording")).toBeInTheDocument();
  });

  it("can end a paused thread and still gets a checkpoint draft", async () => {
    const user = open("PAUSED");
    await user.click(button("End thread…"));
    await user.click(button("End thread and save checkpoint"));
    expect(await screen.findByRole("heading", { name: "Checkpoint draft" })).toBeInTheDocument();
    expect(backend.callsTo("threads_end")).toHaveLength(1);
  });
});

describe("live evidence", () => {
  it("shows a command run through tb while the screen is open", async () => {
    open("ACTIVE", 30);
    const timeline = await screen.findByRole("list", { name: /Evidence timeline/ });
    expect(within(timeline).queryByText("npm test")).toBeNull();
    backend.addExternalCommand("npm test", 1);
    expect(await within(timeline).findByText("npm test")).toBeInTheDocument();
    expect(await within(timeline).findByText("failed (exit code 1) after 1.20 s")).toBeInTheDocument();
  });

  it("does not poll a thread that is not recording", async () => {
    open("PAUSED", 20);
    await screen.findByText(/1 event recorded/);
    const before = backend.callsTo("events_list").length;
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(backend.callsTo("events_list")).toHaveLength(before);
  });
});
