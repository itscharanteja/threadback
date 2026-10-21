import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FakeBackend } from "../test/fakeBackend";
import { App } from "./App";

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

describe("Menu-bar popover application (App.tsx)", () => {
  beforeEach(() => {
    backend = new FakeBackend("ACTIVE");
    backend.config.hasApiKey = true;
    bridge.invoke = backend.invoke;
    bridge.pickedFolder = "/workspace/payments-service";
  });

  it("renders as an anchored popover with brand header and status pill", async () => {
    render(<App />);

    expect(await screen.findByRole("dialog", { name: "ThreadBack menu" })).toBeInTheDocument();
    expect(screen.getByText("ThreadBack")).toBeInTheDocument();
    expect(screen.getByLabelText("Close ThreadBack")).toBeInTheDocument();
    expect(screen.getByText("Stored locally")).toBeInTheDocument();
  });

  it("shows every activity page newest first and refreshes when the popover is focused", async () => {
    const started = backend.events[0];
    const baseTime = Date.parse(started.timestamp);
    for (let index = 1; index <= 105; index += 1) {
      backend.events.push({
        ...started,
        id: `seed-${index}`,
        timestamp: new Date(baseTime + index * 1000).toISOString(),
        payload: { type: "note_added", payload: { text: `Note ${index}`, label: "fact" } },
      });
    }

    render(<App />);
    const activity = await screen.findByRole("list", { name: "Activity, newest first" });
    await waitFor(() => expect(within(activity).getAllByRole("listitem")).toHaveLength(106));
    expect(within(activity).getAllByRole("listitem")[0]).toHaveTextContent("Note 105");
    expect(within(activity).getAllByRole("listitem")[105]).toHaveTextContent("Thread started");
    expect(backend.callsTo("events_list")).toHaveLength(2);

    backend.events.push({
      ...started,
      id: "seed-106",
      timestamp: new Date(baseTime + 106000).toISOString(),
      payload: { type: "note_added", payload: { text: "Note 106", label: "fact" } },
    });
    window.dispatchEvent(new Event("focus"));
    await waitFor(() => expect(within(activity).getAllByRole("listitem")).toHaveLength(107));
    expect(within(activity).getAllByRole("listitem")[0]).toHaveTextContent("Note 106");
  });

  it("dismisses popover on Escape key and on close button click", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("dialog", { name: "ThreadBack menu" });

    // Press Escape
    await user.keyboard("{Escape}");
    expect(backend.callsTo("popover_close")).toHaveLength(1);

    // Click Close Button
    const closeBtn = screen.getByLabelText("Close ThreadBack");
    await user.click(closeBtn);
    expect(backend.callsTo("popover_close")).toHaveLength(2);
  });

  it("manages the complete moment journey: working -> pause -> resume -> save -> review -> draft -> return", async () => {
    const user = userEvent.setup();
    render(<App />);

    // 1. Working moment
    expect(await screen.findByText("Recording this thread")).toBeInTheDocument();
    expect(screen.getByText("Save my place")).toBeInTheDocument();
    expect(screen.getByText("Pause")).toBeInTheDocument();

    // Add a quick note
    const noteInput = screen.getByPlaceholderText("What did you learn?");
    await user.type(noteInput, "Cache TTL causes retry spike");
    await user.click(screen.getByRole("button", { name: "Add note" }));
    expect(await screen.findByText(/Cache TTL causes retry spike/)).toBeInTheDocument();
    expect(backend.callsTo("events_add_note")).toHaveLength(1);

    // Pause recording
    await user.click(screen.getByRole("button", { name: "Pause" }));
    expect(await screen.findByText("Recording paused")).toBeInTheDocument();
    expect(screen.getByText("Your work is still here.")).toBeInTheDocument();
    expect(backend.callsTo("threads_pause")).toHaveLength(1);

    // Resume recording
    await user.click(screen.getByRole("button", { name: "Resume recording" }));
    expect(await screen.findByText("Recording this thread")).toBeInTheDocument();
    expect(backend.callsTo("threads_resume")).toHaveLength(1);

    // Save my place (offline checkpoint)
    await user.click(screen.getByRole("button", { name: "Save my place" }));
    expect(await screen.findByText("Checkpoint saved locally")).toBeInTheDocument();
    expect(screen.getByText("Ready when you return.")).toBeInTheDocument();
    expect(
      screen.getByText(/Nothing has been sent to a model\. You can close ThreadBack now/),
    ).toBeInTheDocument();
    expect(backend.callsTo("checkpoints_save_place")).toHaveLength(1);

    // Review AI sharing
    await user.click(screen.getByRole("button", { name: "Review AI sharing" }));
    expect(await screen.findByText("Review what AI receives")).toBeInTheDocument();
    expect(screen.getByText("Nothing sent yet")).toBeInTheDocument();
    expect(screen.getByText(/KB selected/)).toBeInTheDocument();
    expect(backend.callsTo("checkpoints_preview")).toHaveLength(1);

    // Generate draft summary
    await user.click(screen.getByRole("button", { name: "Generate draft summary" }));
    expect(await screen.findByText("Draft · your approval needed")).toBeInTheDocument();
    expect(screen.getByText("A place to start again.")).toBeInTheDocument();
    expect(backend.callsTo("checkpoints_generate")).toHaveLength(1);

    // Edit next action
    await user.click(screen.getByRole("button", { name: "Edit next action" }));
    const actionEditor = screen.getByLabelText("Edit next action");
    await user.clear(actionEditor);
    await user.type(actionEditor, "Verify cache TTL lock expiration in Redis test");
    await user.click(screen.getByRole("button", { name: "Save edit" }));
    expect(screen.getByText("Verify cache TTL lock expiration in Redis test")).toBeInTheDocument();

    // Approve summary
    await user.click(screen.getByRole("button", { name: "Approve summary" }));
    expect(await screen.findByText("Approved checkpoint")).toBeInTheDocument();
    expect(screen.getByText(/Pick up/)).toBeInTheDocument();
    expect(backend.callsTo("checkpoints_approve")).toHaveLength(1);

    // Copy next action
    await user.click(screen.getByRole("button", { name: "Copy next action" }));
    expect(await screen.findByText("Next action copied. Nothing was run.")).toBeInTheDocument();

    // Resume thread
    await user.click(screen.getByRole("button", { name: "Resume thread" }));
    expect(await screen.findByText("Recording this thread")).toBeInTheDocument();
  });

  it("holds draft generation while a changed sharing selection is being reviewed", async () => {
    const user = userEvent.setup();
    let releasePreview!: () => void;
    const pendingPreview = new Promise<void>((resolve) => { releasePreview = resolve; });
    let previewRequests = 0;
    bridge.invoke = async (command, args) => {
      if (command === "checkpoints_preview" && ++previewRequests === 2) {
        await pendingPreview;
      }
      return backend.invoke(command, args);
    };

    render(<App />);
    await screen.findByText("Recording this thread");
    await user.type(screen.getByPlaceholderText("What did you learn?"), "Retry starts a second capture");
    await user.click(screen.getByRole("button", { name: "Add note" }));
    await user.click(screen.getByRole("button", { name: "Save my place" }));
    await user.click(await screen.findByRole("button", { name: "Review AI sharing" }));
    await screen.findByText("Review what AI receives");

    await user.click(screen.getAllByRole("checkbox")[0]);
    expect(screen.getByText("Updating the exact payload…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Generate draft summary" })).toBeDisabled();
    expect(backend.callsTo("checkpoints_generate")).toHaveLength(0);

    releasePreview();
    await waitFor(() => expect(screen.getByRole("button", { name: "Generate draft summary" })).toBeEnabled());
  });

  it("navigates between Home, Threads list, and Privacy & settings", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("dialog", { name: "ThreadBack menu" });

    // Click Threads in footer
    await user.click(screen.getByRole("button", { name: "Threads" }));
    expect(await screen.findByText("Your threads")).toBeInTheDocument();
    expect(screen.getByText("Start a thread…")).toBeInTheDocument();

    // Click Privacy in footer
    await user.click(screen.getByRole("button", { name: "Privacy" }));
    expect(await screen.findByText("Privacy & settings")).toBeInTheDocument();
    expect(screen.getByText("Local by default")).toBeInTheDocument();
    expect(screen.getByText("Approved project only")).toBeInTheDocument();
    expect(screen.getByText("Explicit capture")).toBeInTheDocument();
    expect(screen.getByText("Local checkpoints")).toBeInTheDocument();
    expect(screen.getByText("AI requests require review")).toBeInTheDocument();
    expect(screen.getByText("Quit ThreadBack")).toBeInTheDocument();

    // Click Quit ThreadBack
    await user.click(screen.getByRole("button", { name: "Quit ThreadBack" }));
    expect(backend.callsTo("app_quit")).toHaveLength(1);

    // Click Home to return
    await user.click(screen.getByRole("button", { name: "Home" }));
    expect(await screen.findByText("Recording this thread")).toBeInTheDocument();
  });

  it("supports starting a new thread with validation", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("dialog", { name: "ThreadBack menu" });
    await user.click(screen.getByRole("button", { name: "Threads" }));
    await user.click(screen.getByRole("button", { name: "Start a thread…" }));

    expect(await screen.findByText("Start a thread")).toBeInTheDocument();

    // Validation
    await user.click(screen.getByRole("button", { name: "Start recording" }));
    expect(await screen.findByText("Give the thread a title.")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Title"), "Lock owner race condition");
    await user.type(screen.getByLabelText("Intended outcome"), "Confirm lock release semantics");
    await user.click(screen.getByRole("button", { name: "Start recording" }));

    expect(backend.callsTo("threads_start")).toHaveLength(1);
    expect(await screen.findByText("Lock owner race condition")).toBeInTheDocument();
    expect(screen.getByText("Recording this thread")).toBeInTheDocument();
  });

  it("chooses a new folder while creating a thread and keeps the draft through approval", async () => {
    bridge.pickedFolder = "/workspace/another-repo";
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("dialog", { name: "ThreadBack menu" });
    await user.click(screen.getByRole("button", { name: "New thread" }));
    await user.type(screen.getByLabelText("Title"), "Investigate a new repository");
    await user.type(screen.getByLabelText("Intended outcome"), "Find the failing build step");
    await user.click(screen.getByRole("button", { name: "Choose another folder…" }));

    expect(await screen.findByRole("heading", { name: "Approve a project" })).toBeInTheDocument();
    expect(screen.getByText("/workspace/another-repo")).toBeInTheDocument();
    expect(screen.getByLabelText("Project name")).toHaveValue("another-repo");

    await user.click(screen.getByLabelText(/Confirm ThreadBack may collect/));
    await waitFor(() => expect(screen.getByRole("button", { name: "Approve project" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "Approve project" }));

    expect(await screen.findByRole("heading", { name: "Start a thread" })).toBeInTheDocument();
    expect(screen.getByLabelText("Project folder")).toHaveValue("project-2");
    expect(screen.getByLabelText("Title")).toHaveValue("Investigate a new repository");
    expect(screen.getByLabelText("Intended outcome")).toHaveValue("Find the failing build step");

    await user.click(screen.getByRole("button", { name: "Start recording" }));
    expect(backend.callsTo("threads_start")[0].args.input.projectId).toBe("project-2");
  });

  it("explains missing Git and initializes it only after the user chooses to", async () => {
    bridge.pickedFolder = "/workspace/plain-folder";
    backend.gitlessFolders.add(bridge.pickedFolder);
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("dialog", { name: "ThreadBack menu" });
    await user.click(screen.getByRole("button", { name: "New thread" }));
    await user.click(screen.getByRole("button", { name: "Choose another folder…" }));

    expect(await screen.findByText("This folder needs Git before you can start a thread.")).toBeInTheDocument();
    expect(screen.getByText(/adds a .git folder; it does not commit your files/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Approve project" })).toBeDisabled();
    expect(backend.callsTo("projects_initialize_git")).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "Initialize Git in this folder" }));
    expect(backend.callsTo("projects_initialize_git")).toHaveLength(1);
    expect(await screen.findByText("Git repository ready.")).toBeInTheDocument();

    await user.click(screen.getByLabelText(/Confirm ThreadBack may collect/));
    await user.click(screen.getByRole("button", { name: "Approve project" }));
    expect(await screen.findByRole("heading", { name: "Start a thread" })).toBeInTheDocument();
    expect(screen.getByLabelText("Project folder")).toHaveValue("project-2");
  });

  it("selects an already approved folder without asking for approval again", async () => {
    backend.projectsList.push({
      id: "project-2",
      displayName: "Another repository",
      canonicalRoot: "/workspace/another-repo",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    });
    bridge.pickedFolder = "/workspace/another-repo/";
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("dialog", { name: "ThreadBack menu" });
    await user.click(screen.getByRole("button", { name: "New thread" }));
    await user.click(screen.getByRole("button", { name: "Choose another folder…" }));

    expect(screen.getByRole("heading", { name: "Start a thread" })).toBeInTheDocument();
    expect(screen.getByLabelText("Project folder")).toHaveValue("project-2");
    expect(backend.callsTo("projects_create")).toHaveLength(0);
  });

  it("keeps thread creation visible while viewing a paused thread", async () => {
    backend = new FakeBackend("PAUSED");
    bridge.invoke = backend.invoke;
    const user = userEvent.setup();
    render(<App />);

    expect(await screen.findByText("Recording paused")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New thread" }));
    expect(await screen.findByRole("heading", { name: "Start a thread" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Threads" }));
    const start = screen.getByRole("button", { name: "Start a thread…" });
    const firstThread = screen.getByRole("button", { name: /Duplicate capture investigation/ });
    expect(start.compareDocumentPosition(firstThread) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("explains why the browser preview cannot create a thread", async () => {
    bridge.invoke = async () => { throw new Error("Tauri bridge unavailable"); };
    render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Open the ThreadBack macOS app to create threads",
    );
    expect(screen.getByRole("button", { name: "New thread" })).toBeDisabled();
  });

  it("supports approving a project folder with system dialog and consent", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("dialog", { name: "ThreadBack menu" });
    await user.click(screen.getByRole("button", { name: "Threads" }));
    await user.click(screen.getByRole("button", { name: "Start a thread…" }));

    // Click Cancel to test cancel navigation
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(await screen.findByText("Your threads")).toBeInTheDocument();
  });
});
