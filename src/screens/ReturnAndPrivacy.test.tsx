import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { Project } from "../bindings/contracts";
import { RecoveryBanner } from "../components/RecoveryBanner";
import { StatusBar } from "../components/StatusBar";
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

describe("failure states and recovery UI (R-04a)", () => {
  it("shows missing artifact last known relative path when a referenced file no longer exists", async () => {
    const user = await openApprovedCapsule();
    backend.artifactFiles.clear();

    await user.click(screen.getByRole("button", { name: "Open file tests/payment-retry.test.mjs" }));
    expect(backend.callsTo("checkpoints_open_artifact")).toHaveLength(1);
    expect(
      await screen.findByText(/Artifact missing on disk; showing last known relative path \(tests\/payment-retry\.test\.mjs\)\./),
    ).toBeInTheDocument();
    expect(screen.getByText(/Artifact missing on disk\. Last known relative path:/)).toBeInTheDocument();
  });

  it("keeps the approved capsule readable when a project moved and lets the user relink the project folder", async () => {
    const user = await openApprovedCapsule([
      {
        kind: "repository_unavailable",
        label: "Could not inspect current Git state at /workspace/payments-old.",
        before: "/workspace/payments-old",
        after: null,
      },
    ]);

    expect(screen.getByRole("heading", { name: "Approved resume capsule" })).toBeInTheDocument();
    expect(screen.getByText(/Could not inspect current Git state/)).toBeInTheDocument();

    const relinkInput = screen.getByLabelText("Relink moved project folder");
    await user.clear(relinkInput);
    await user.type(relinkInput, "/workspace/payments-relinked");
    await user.click(screen.getByRole("button", { name: "Relink project folder" }));

    expect(backend.callsTo("projects_relink")).toHaveLength(1);
    expect(await screen.findByText(/Relinked project folder to \/workspace\/payments-relinked/)).toBeInTheDocument();
    expect(screen.getByText("No drift:")).toBeInTheDocument();
    expect(screen.getByText(/Repository matches the checkpoint snapshot/)).toBeInTheDocument();
  });

  it("renders crash recovery reports with last acknowledged event timestamp and dismisses cleanly", async () => {
    const onDismiss = vi.fn();
    const user = userEvent.setup();
    render(
      <RecoveryBanner
        reports={[
          {
            threadId: "thread-1",
            threadTitle: "Duplicate capture investigation",
            lastEventAt: "2026-10-07T10:15:00.000Z",
            recoveredAt: "2026-10-07T10:20:00.000Z",
          },
        ]}
        onDismiss={onDismiss}
      />,
    );

    const banner = screen.getByRole("status", { name: "Interrupted threads" });
    expect(within(banner).getByText("Duplicate capture investigation")).toBeInTheDocument();
    expect(within(banner).getByText(/was still recording when ThreadBack last stopped, so it was paused/)).toBeInTheDocument();
    expect(within(banner).getByText(/nothing was captured between then and/)).toBeInTheDocument();

    await user.click(within(banner).getByRole("button", { name: "Dismiss" }));
    expect(onDismiss).toHaveBeenCalledWith("thread-1");
  });
});

describe("keyboard, focus, semantic labels, and non-color-only status indicators (R-04b)", () => {
  it("communicates recording, idle, online, and offline states with explicit words and glyphs (never color alone)", () => {
    const now = new Date().toISOString();
    const { rerender } = render(
      <StatusBar
        coreReady={true}
        online={true}
        recordingThread={{
          id: "thread-1",
          projectId: "project-1",
          title: "Duplicate capture investigation",
          goal: "Stop duplicate capture",
          state: "ACTIVE",
          startedAt: now,
          endedAt: null,
          createdAt: now,
          updatedAt: now,
        }}
        onOpenThread={() => {}}
      />,
    );

    const bar = screen.getByRole("region", { name: "Status" });
    expect(within(bar).getByText(/Capturing evidence:/)).toBeInTheDocument();
    expect(within(bar).getByText("●")).toBeInTheDocument();
    expect(within(bar).getByText(/Network available/)).toBeInTheDocument();
    expect(within(bar).getByText(/Local data ready/)).toBeInTheDocument();

    rerender(<StatusBar coreReady={false} online={false} recordingThread={undefined} onOpenThread={() => {}} />);
    expect(within(bar).getByText(/Not capturing — nothing is recorded without an active thread/)).toBeInTheDocument();
    expect(within(bar).getByText("○")).toBeInTheDocument();
    expect(within(bar).getByText(/Offline — checkpoints still work locally/)).toBeInTheDocument();
    expect(within(bar).getByText("✕")).toBeInTheDocument();
    expect(within(bar).getByText(/Starting local data/)).toBeInTheDocument();
  });

  it("supports keyboard-only note entry (Ctrl+Enter) and accessible semantic labels across ThreadScreen", async () => {
    backend = new FakeBackend("ACTIVE");
    bridge.invoke = backend.invoke;
    const user = userEvent.setup();
    render(<ThreadHarness />);

    // Select a note label via keyboard/click and submit via Ctrl+Enter
    const hypothesisRadio = screen.getByRole("radio", { name: /^Hypothesis/ });
    await user.click(hypothesisRadio);
    const noteInput = screen.getByRole("textbox", { name: /^Note/ });
    await user.click(noteInput);
    await user.keyboard("Keyboard-submitted hypothesis{Control>}{Enter}{/Control}");

    expect(backend.callsTo("events_add_note")).toHaveLength(1);
    expect(await screen.findByText("Keyboard-submitted hypothesis")).toBeInTheDocument();

    // Verify semantic landmarks exist for screen-reader navigation
    expect(screen.getByRole("region", { name: "Goal" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Thread actions" })).toBeInTheDocument();
    expect(screen.getByRole("form", { name: "Add a note" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Evidence" })).toBeInTheDocument();
  });
});

