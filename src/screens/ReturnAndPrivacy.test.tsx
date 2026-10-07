import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { Project } from "../bindings/contracts";
import { FakeBackend } from "../test/fakeBackend";
import { PrivacyScreen } from "./PrivacyScreen";
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
  canonicalRoot: "/workspace/payments",
  createdAt: new Date().toISOString(),
  updatedAt: new Date().toISOString(),
};

let backend: FakeBackend;

function ThreadHarness({ onDeleted = () => {} }: { onDeleted?: () => void }) {
  const [thread, setThread] = useState(backend.thread);
  return (
    <ThreadScreen
      thread={thread}
      project={project}
      refreshMs={60_000}
      onChanged={async () => setThread({ ...backend.thread })}
      onDeleted={onDeleted}
    />
  );
}

async function openApprovedCapsule(driftSignals: { kind: string; label: string; before: string | null; after: string | null }[] = []) {
  backend = new FakeBackend("ACTIVE");
  bridge.invoke = backend.invoke;
  backend.config.hasApiKey = true;
  if (driftSignals.length > 0) {
    backend.drift = {
      checked: true,
      hasDrift: true,
      summary: `Repository changed since this checkpoint (${driftSignals.length} differences).`,
      signals: driftSignals,
      checkpointSnapshot: null,
      currentSnapshot: null,
    };
  }
  await backend.invoke("events_add_note", {
    threadId: "thread-1",
    note: { text: "The focused test made two provider calls.", label: "fact" },
  });
  const user = userEvent.setup();
  render(<ThreadHarness />);
  await user.click(screen.getByRole("button", { name: "Save My Place" }));
  await user.click(await screen.findByRole("button", { name: "Review what will be sent…" }));
  await screen.findByRole("list", { name: "Entries in the payload" });
  await user.click(screen.getByRole("button", { name: /Generate capsule/ }));
  await screen.findByRole("heading", { name: "Resume capsule (draft)" });
  await user.click(screen.getByRole("button", { name: /Approve capsule/ }));
  await screen.findByRole("heading", { name: "Approved resume capsule" });
  return user;
}

describe("returning to an approved capsule (U-04)", () => {
  it("shows repository drift signals (branch, HEAD, status) before resume actions", async () => {
    await openApprovedCapsule([
      { kind: "branch_changed", label: "Branch changed from main to fix/retry.", before: "main", after: "fix/retry" },
      { kind: "head_changed", label: "HEAD commit changed from 1111111 to 9999999.", before: "1111111", after: "9999999" },
      { kind: "working_tree_changed", label: "Working tree status changed (clean → 1 changed: src/retry.rs).", before: "clean", after: "1 changed: src/retry.rs" },
    ]);

    expect(screen.getByRole("heading", { name: "Repository state since checkpoint" })).toBeInTheDocument();
    expect(screen.getByText(/Drift detected:/)).toBeInTheDocument();
    const list = screen.getByRole("list", { name: "Repository drift signals" });
    expect(within(list).getByText(/Branch changed from main to fix\/retry\./)).toBeInTheDocument();
    expect(within(list).getByText(/HEAD commit changed from 1111111 to 9999999\./)).toBeInTheDocument();
    expect(within(list).getByText(/Working tree status changed/)).toBeInTheDocument();
  });

  it("requires explicit clicks to copy the next action or open an artifact, and never runs commands", async () => {
    const user = await openApprovedCapsule();

    // No artifact is opened automatically on load.
    expect(backend.callsTo("checkpoints_open_artifact")).toHaveLength(0);
    expect(screen.queryByRole("heading", { name: /Opened file:/ })).toBeNull();

    // Copy next action requires an explicit click.
    await user.click(screen.getByRole("button", { name: "Copy next action" }));
    expect(await screen.findByText(/Copied next action to clipboard|Next action ready to copy/)).toBeInTheDocument();

    // Opening an artifact requires an explicit click.
    await user.click(screen.getByRole("button", { name: "Open file tests/payment-retry.test.mjs" }));
    expect(backend.callsTo("checkpoints_open_artifact")).toHaveLength(1);
    expect(await screen.findByRole("heading", { name: "Opened file: tests/payment-retry.test.mjs" })).toBeInTheDocument();
    expect(screen.getByText(/Focused payment retry test/)).toBeInTheDocument();
  });

  it("exports approved JSON and Markdown, resumes, archives, and deletes with confirmation", async () => {
    const user = await openApprovedCapsule();

    await user.click(screen.getByRole("button", { name: "Export JSON" }));
    expect(backend.callsTo("exports_json")).toHaveLength(1);
    expect(await screen.findByText(/Exported approved capsule as JSON/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Export Markdown" }));
    expect(backend.callsTo("exports_markdown")).toHaveLength(1);
    expect(await screen.findByText(/Exported approved capsule as MARKDOWN/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Archive thread" }));
    expect(backend.callsTo("threads_archive")).toHaveLength(1);
    expect(backend.thread.state).toBe("ARCHIVED");

    await user.click(screen.getByRole("button", { name: "Delete thread…" }));
    const confirmHeading = await screen.findByRole("heading", { name: "Permanently delete this thread?" });
    await waitFor(() => expect(confirmHeading).toHaveFocus());
    await user.click(screen.getByRole("button", { name: "Delete thread permanently" }));
    expect(backend.callsTo("threads_delete")).toHaveLength(1);
    expect(backend.deletedThreads).toContain("thread-1");
  });
});

describe("privacy, retention, redaction tester, and pause-all controls (U-05)", () => {
  it("updates retention and collector toggles, runs redaction tests, pauses active capture, and deletes threads", async () => {
    backend = new FakeBackend("ACTIVE");
    bridge.invoke = backend.invoke;
    const user = userEvent.setup();

    render(
      <PrivacyScreen
        projects={backend.projectsList}
        threads={[backend.thread]}
        recordingThread={backend.thread}
      />,
    );

    expect(await screen.findByText(/fixtures\/sensitive\/\*\*/)).toBeInTheDocument();

    const retentionInput = screen.getByLabelText(/Raw event retention \(days\)/i);
    await user.clear(retentionInput);
    await user.type(retentionInput, "30");
    await user.click(screen.getByRole("button", { name: "Save privacy policy" }));
    expect(backend.callsTo("projects_update_policy")).toHaveLength(1);
    expect(backend.policy.retentionDays).toBe(30);
    expect(await screen.findByText(/Saved project privacy policy/)).toBeInTheDocument();

    // Live redaction test
    await user.click(screen.getByRole("button", { name: "Run redaction test" }));
    expect(backend.callsTo("privacy_test_redaction")).toHaveLength(1);
    const output = await screen.findByRole("region", { name: "Redaction test output" });
    expect(within(output).getByText("Authorization: Bearer [REDACTED:bearer-token]")).toBeInTheDocument();

    // Pause all active capture
    await user.click(screen.getByRole("button", { name: "Pause all active capture" }));
    expect(backend.callsTo("threads_pause_all")).toHaveLength(1);
    expect(backend.thread.state).toBe("PAUSED");

    // Destructive thread deletion from Privacy screen
    await user.click(screen.getByRole("button", { name: /Delete "Duplicate capture investigation"/ }));
    await screen.findByRole("heading", { name: /Permanently delete "Duplicate capture investigation"\?/ });
    await user.click(screen.getByRole("button", { name: "Confirm permanent deletion" }));
    expect(backend.callsTo("threads_delete")).toHaveLength(1);
  });
});
