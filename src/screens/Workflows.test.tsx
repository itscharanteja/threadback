import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { Project, WorkThread } from "../bindings/contracts";
import { FakeBackend } from "../test/fakeBackend";
import { ApproveProjectScreen } from "./ApproveProjectScreen";
import { HomeScreen } from "./HomeScreen";
import { NewThreadScreen } from "./NewThreadScreen";
import { ThreadScreen } from "./ThreadScreen";

const bridge = vi.hoisted(() => ({
  invoke: null as null | ((command: string, args?: unknown) => Promise<unknown>),
  pickedFolder: "/workspace/payments-service" as string | null,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => bridge.invoke!(command, args),
}));

vi.mock("../api/dialog", () => ({
  chooseProjectFolder: async () => bridge.pickedFolder,
}));

let backend: FakeBackend;

describe("onboarding, home, and new-thread workflows (U-01)", () => {
  it("validates folder, project name, and explicit consent before approving a project", async () => {
    backend = new FakeBackend("ACTIVE");
    bridge.invoke = backend.invoke;
    bridge.pickedFolder = "/workspace/payments-service";
    const onApproved = vi.fn();
    const user = userEvent.setup();

    render(<ApproveProjectScreen onApproved={onApproved} onCancel={() => {}} />);

    // Submitting empty form shows field errors for folder, name, and consent
    await user.click(screen.getByRole("button", { name: "Approve project" }));
    expect(screen.getByText("Choose the project folder first.")).toBeInTheDocument();
    expect(screen.getByText("Give the project a name.")).toBeInTheDocument();
    expect(screen.getByText(/Confirm that ThreadBack may collect evidence inside this folder/)).toBeInTheDocument();
    expect(backend.callsTo("projects_create")).toHaveLength(0);

    // Pick folder (auto-fills folder name) and check consent
    await user.click(screen.getByRole("button", { name: /^Project folder/ }));
    expect(screen.getByDisplayValue("payments-service")).toBeInTheDocument();

    const consentBox = screen.getByRole("checkbox", { name: /^Consent/ });
    await user.click(consentBox);
    await user.click(screen.getByRole("button", { name: "Approve project" }));

    expect(backend.callsTo("projects_create")).toHaveLength(1);
    expect(onApproved).toHaveBeenCalledWith(
      expect.objectContaining({
        displayName: "payments-service",
        canonicalRoot: "/workspace/payments-service",
      }),
    );
  });

  it("refuses to start a thread without a title and intended outcome (goal)", async () => {
    backend = new FakeBackend("ACTIVE");
    bridge.invoke = backend.invoke;
    const onStarted = vi.fn();
    const user = userEvent.setup();

    render(
      <NewThreadScreen
        projects={backend.projectsList}
        onStarted={onStarted}
        onApproveProject={() => {}}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Start thread" }));
    expect(screen.getByText("Give the thread a title.")).toBeInTheDocument();
    expect(screen.getByText("Describe the outcome you are working toward.")).toBeInTheDocument();
    expect(backend.callsTo("threads_start")).toHaveLength(0);

    await user.type(screen.getByRole("textbox", { name: /^Title/ }), "Webhook signature check");
    await user.type(
      screen.getByRole("textbox", { name: /^Intended outcome/ }),
      "Find why chunked webhook payloads fail HMAC verification",
    );
    await user.type(
      screen.getByRole("textbox", { name: /^Starting hypothesis/ }),
      "Reverse proxy strips trailing newline on chunked transfer",
    );
    await user.click(screen.getByRole("button", { name: "Start thread" }));

    expect(backend.callsTo("threads_start")).toHaveLength(1);
    expect(onStarted).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "Webhook signature check",
        goal: "Find why chunked webhook payloads fail HMAC verification",
        state: "ACTIVE",
      }),
    );
  });

  it("renders approved projects and recent threads with explicit state labels on HomeScreen", async () => {
    backend = new FakeBackend("ACTIVE");
    bridge.invoke = backend.invoke;
    const onOpenThread = vi.fn();
    const onStartThread = vi.fn();
    const user = userEvent.setup();

    render(
      <HomeScreen
        projects={backend.projectsList}
        threads={[backend.thread]}
        loadError={null}
        onApproveProject={() => {}}
        onStartThread={onStartThread}
        onOpenThread={onOpenThread}
      />,
    );

    expect(screen.getByRole("heading", { name: "Approved projects" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Recent threads" })).toBeInTheDocument();
    expect(screen.getByText(/Active — recording evidence/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Start thread\s*in Payments/ }));
    expect(onStartThread).toHaveBeenCalledWith(backend.projectsList[0]);

    await user.click(screen.getByRole("button", { name: /Open\s*Duplicate capture investigation/ }));
    expect(onOpenThread).toHaveBeenCalledWith(backend.thread);
  });
});

describe("multi-stage persona workflow in ThreadScreen (W01/W02/W03)", () => {
  it("completes note capture -> offline save -> draft discard -> second save -> preview -> generate -> relabel -> approve -> drift resume", async () => {
    backend = new FakeBackend("ACTIVE");
    bridge.invoke = backend.invoke;
    backend.config.hasApiKey = true;
    backend.drift = {
      checked: true,
      hasDrift: true,
      summary: "Repository changed since this checkpoint (1 difference).",
      signals: [
        {
          kind: "branch_changed",
          label: "Branch changed from main to hotfix/review-feedback.",
          before: "main",
          after: "hotfix/review-feedback",
        },
      ],
      checkpointSnapshot: null,
      currentSnapshot: null,
    };

    const project: Project = backend.projectsList[0];
    function WorkflowHarness() {
      const [thread, setThread] = useState<WorkThread>(backend.thread);
      return (
        <ThreadScreen
          thread={thread}
          project={project}
          refreshMs={60_000}
          onChanged={async () => setThread({ ...backend.thread })}
          onDeleted={() => {}}
        />
      );
    }

    const user = userEvent.setup();
    render(<WorkflowHarness />);

    // 1. Add a fact note and Save My Place offline
    await user.click(screen.getByRole("radio", { name: /^Fact/ }));
    await user.type(
      screen.getByRole("textbox", { name: /^Note/ }),
      "The focused retry test made two provider calls.",
    );
    await user.click(screen.getByRole("button", { name: "Add note" }));
    await user.click(screen.getByRole("button", { name: "Save My Place" }));
    expect(await screen.findByRole("heading", { name: "Checkpoint draft" })).toBeInTheDocument();

    // 2. Discard the draft to keep working (Arjun workflow)
    await user.click(screen.getByRole("button", { name: "Discard draft and keep working…" }));
    await user.click(await screen.findByRole("button", { name: "Discard draft" }));
    expect(backend.callsTo("checkpoints_discard")).toHaveLength(1);
    expect(await screen.findByText(/Active — recording evidence/)).toBeInTheDocument();

    // 3. Save My Place again, preview outbound payload, and generate capsule
    await user.click(screen.getByRole("button", { name: "Save My Place" }));
    await user.click(await screen.findByRole("button", { name: "Review what will be sent…" }));
    await screen.findByRole("list", { name: "Entries in the payload" });
    await user.click(screen.getByRole("button", { name: /Generate capsule/ }));
    await screen.findByRole("heading", { name: "Resume capsule (draft)" });

    // 4. Relabel a verified fact into a hypothesis during review (Priya workflow) and approve
    const factsSection = screen.getByRole("region", { name: "Verified facts" });
    await user.click(within(factsSection).getByRole("button", { name: "Make fact 1 a hypothesis" }));
    await user.click(screen.getByRole("button", { name: /Approve capsule/ }));

    // 5. Inspect drift on ApprovedCapsule and resume thread (Maya workflow)
    await screen.findByRole("heading", { name: "Approved resume capsule" });
    expect(screen.getByText(/Branch changed from main to hotfix\/review-feedback\./)).toBeInTheDocument();
    expect(screen.getAllByText(/Capsule approved with 2 recorded corrections\./).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "Resume thread" }));
    expect(backend.callsTo("threads_resume")).toHaveLength(1);
    expect(await screen.findByText(/Resumed — recording evidence|Active — recording evidence/)).toBeInTheDocument();
  });
});
