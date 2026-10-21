import { useEffect, useRef, useState, type FormEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  Checkpoint,
  EventId,
  PayloadPreview,
  Project,
  ProjectPolicy,
  RedactionTestResult,
  ResumeViewModel,
  ThreadEvent,
  WorkThread,
} from "../bindings/contracts";
import {
  checkpoints as checkpointsApi,
  configuration as configApi,
  events as eventsApi,
  exports as exportsApi,
  popover as popoverApi,
  privacy as privacyApi,
  projects as projectsApi,
  threads as threadsApi,
} from "../api/commands";
import { chooseProjectFolder } from "../api/dialog";
import { toDomainError } from "../api/errors";
import { folderName, formatTime, isRecording } from "../lib/format";
import { useAppData } from "../state/useAppData";
import evidenceListIcon from "../../src-tauri/icons/app-icon.svg";

export type PopoverMoment =
  | { name: "working"; threadId: string }
  | { name: "paused"; threadId: string }
  | { name: "saved"; threadId: string; checkpointId: string }
  | { name: "review"; threadId: string; checkpointId: string }
  | { name: "draft"; threadId: string; checkpointId: string }
  | { name: "return"; threadId: string; checkpointId: string }
  | { name: "threads" }
  | { name: "new-thread"; projectId?: string }
  | { name: "approve-project" }
  | { name: "settings" };

function newestFirst(items: ThreadEvent[]): ThreadEvent[] {
  return [...items].sort((a, b) =>
    Date.parse(b.timestamp) - Date.parse(a.timestamp) || b.id.localeCompare(a.id),
  );
}

function activityLabel(event: ThreadEvent): string {
  switch (event.payload.type) {
    case "note_added": {
      const { label, text } = event.payload.payload;
      return `${label.toUpperCase()}: ${text}`;
    }
    case "thread_started": return "Thread started";
    case "thread_paused": return "Recording paused";
    case "thread_resumed": return "Recording resumed";
    case "git_snapshot": return "Git snapshot captured";
    case "file_changed": return "File change recorded";
    case "command_started": {
      const payload = event.payload.payload;
      const command = typeof payload === "object" && payload !== null && !Array.isArray(payload)
        ? payload.command : null;
      return typeof command === "string" ? `Command started: ${command}` : "Command started";
    }
    case "command_finished": {
      const payload = event.payload.payload;
      const exitCode = typeof payload === "object" && payload !== null && !Array.isArray(payload)
        ? payload.exitCode : null;
      return typeof exitCode === "number" ? `Command finished (exit ${exitCode})` : "Command finished";
    }
    case "checkpoint_requested": return "Checkpoint saved";
    case "checkpoint_generated": return "Draft generated";
    case "checkpoint_approved": return "Checkpoint approved";
    case "checkpoint_discarded": return "Draft discarded";
  }
}

export function App() {
  const data = useAppData();
  const [view, setView] = useState<PopoverMoment>({ name: "threads" });
  const [lastMainView, setLastMainView] = useState<PopoverMoment>({ name: "threads" });
  const [initialized, setInitialized] = useState(false);

  // Active moment data
  const [events, setEvents] = useState<ThreadEvent[]>([]);
  const [eventsLoading, setEventsLoading] = useState(false);
  const [eventsError, setEventsError] = useState<string | null>(null);
  const [activityRefresh, setActivityRefresh] = useState(0);
  const loadedEventsThread = useRef<string | null>(null);
  const lastActivityRefresh = useRef(0);
  const [activeCheckpoint, setActiveCheckpoint] = useState<Checkpoint | null>(null);
  const [preview, setPreview] = useState<PayloadPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const previewRequest = useRef(0);
  const [excludedIds, setExcludedIds] = useState<EventId[]>([]);
  const [resumeData, setResumeData] = useState<ResumeViewModel | null>(null);
  const [generating, setGenerating] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const toastTimer = useRef<number | null>(null);
  const popoverRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let animation: Animation | undefined;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    let lastPlayedAt = 0;
    const refreshActivity = () => {
      const now = performance.now();
      if (now - lastActivityRefresh.current < 200) return;
      lastActivityRefresh.current = now;
      setActivityRefresh((current) => current + 1);
    };
    const playOpeningMotion = () => {
      refreshActivity();
      const now = performance.now();
      if (now - lastPlayedAt < 120 || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
      lastPlayedAt = now;
      animation?.cancel();
      animation = popoverRef.current?.animate?.(
        [
          { opacity: 0, transform: "translateY(-8px) scale(0.985)" },
          { opacity: 1, transform: "translateY(0) scale(1)" },
        ],
        { duration: 210, easing: "cubic-bezier(.2, .8, .2, 1)" },
      );
    };

    window.addEventListener("focus", playOpeningMotion);
    if ("__TAURI_INTERNALS__" in window) {
      listen("popover-opened", playOpeningMotion)
        .then((dispose) => {
          if (disposed) dispose();
          else unlisten = dispose;
        })
        .catch(() => {});
    }
    return () => {
      disposed = true;
      window.removeEventListener("focus", playOpeningMotion);
      unlisten?.();
      animation?.cancel();
    };
  }, []);

  // Quick note form
  const [noteKind, setNoteKind] = useState<"fact" | "hypothesis" | "question" | "next_step">("fact");
  const [noteText, setNoteText] = useState("");

  // Next action editing
  const [editingAction, setEditingAction] = useState(false);
  const [actionText, setActionText] = useState("");
  const [showSources, setShowSources] = useState(false);

  // New thread form
  const [newProjectId, setNewProjectId] = useState("");
  const [newTitle, setNewTitle] = useState("");
  const [newGoal, setNewGoal] = useState("");
  const [newHypothesis, setNewHypothesis] = useState("");
  const [newThreadErrors, setNewThreadErrors] = useState<Record<string, string>>({});

  // Approve project form
  const [projectPath, setProjectPath] = useState<string | null>(null);
  const [projectName, setProjectName] = useState("");
  const [projectConsent, setProjectConsent] = useState(false);
  const [projectGitState, setProjectGitState] = useState<"idle" | "checking" | "ready" | "needs-init" | "initializing" | "error">("idle");
  const projectGitCheck = useRef(0);
  const [defaultExclusions, setDefaultExclusions] = useState<string[]>([]);
  const [approveErrors, setApproveErrors] = useState<Record<string, string>>({});

  // Model & privacy settings
  const [modelEndpoint, setModelEndpoint] = useState("https://api.tokenfactory.nebius.com/v1");
  const [modelId, setModelId] = useState("nvidia/nemotron-3-super-120b-a12b");
  const [modelApiKey, setModelApiKey] = useState("");
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [redactionInput, setRedactionInput] = useState("Authorization: Bearer nbf_demo_7F4A91C2_DO_NOT_SEND");
  const [redactionResult, setRedactionResult] = useState<RedactionTestResult | null>(null);

  function toast(message: string) {
    setToastMessage(message);
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToastMessage(null), 3400);
  }

  // Sync menu bar status whenever view or data changes
  useEffect(() => {
    let status: "working" | "saved" | "attention" | "idle" = "idle";
    if (view.name === "working") {
      status = "working";
    } else if (view.name === "return") {
      status = resumeData?.drift?.hasDrift ? "attention" : "saved";
    } else if (["saved", "paused", "review", "draft"].includes(view.name)) {
      status = "saved";
    } else if (data.recordingThread) {
      status = "working";
    }
    popoverApi.setStatus(status).catch(() => {});
  }, [view.name, data.recordingThread, resumeData?.drift?.hasDrift]);

  // Focus view title for keyboard and screen-readers
  useEffect(() => {
    document.getElementById("view-title")?.focus();
  }, [view.name]);

  // Handle Esc key to close popover
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        popoverApi.close().catch(() => {});
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  // Initialize initial moment based on local data state
  useEffect(() => {
    if (!data.loaded || initialized) return;
    setInitialized(true);

    if (data.recordingThread) {
      const t = data.recordingThread;
      if (t.state === "ACTIVE") {
        setView({ name: "working", threadId: t.id });
        setLastMainView({ name: "working", threadId: t.id });
      } else if (t.state === "PAUSED") {
        setView({ name: "paused", threadId: t.id });
        setLastMainView({ name: "paused", threadId: t.id });
      } else {
        loadLatestCheckpoint(t.id, t.state);
      }
    } else if (data.threads.length > 0) {
      const latest = data.threads[0];
      if (latest.state === "READY") {
        loadLatestCheckpoint(latest.id, "READY");
      } else if (latest.state === "PAUSED") {
        setView({ name: "paused", threadId: latest.id });
        setLastMainView({ name: "paused", threadId: latest.id });
      } else {
        setView({ name: "threads" });
      }
    } else if (data.projects.length === 0) {
      setView({ name: "approve-project" });
    } else {
      setView({ name: "new-thread", projectId: data.projects[0].id });
    }
  }, [data.loaded, data.recordingThread, data.threads, data.projects, initialized]);

  async function loadLatestCheckpoint(threadId: string, state: string) {
    try {
      const list = await checkpointsApi.list(threadId);
      const cp = list.find((c) => c.status !== "discarded") ?? list[0];
      if (cp) {
        setActiveCheckpoint(cp);
        if (state === "READY" || cp.status === "approved") {
          const res = await checkpointsApi.getResume(cp.id);
          setResumeData(res);
          setActionText(res.checkpoint.approved?.next_action?.instruction ?? "");
          setView({ name: "return", threadId, checkpointId: cp.id });
          setLastMainView({ name: "return", threadId, checkpointId: cp.id });
        } else if (cp.draft) {
          setActionText(cp.draft.next_action?.instruction ?? "");
          setView({ name: "draft", threadId, checkpointId: cp.id });
          setLastMainView({ name: "draft", threadId, checkpointId: cp.id });
        } else {
          setView({ name: "saved", threadId, checkpointId: cp.id });
          setLastMainView({ name: "saved", threadId, checkpointId: cp.id });
        }
      } else {
        setView({ name: "threads" });
      }
    } catch {
      setView({ name: "threads" });
    }
  }

  // The native timeline is paginated oldest first. Load every page for this thread,
  // then present the complete activity newest first.
  useEffect(() => {
    if (!("threadId" in view) || !view.threadId) return;
    const threadId = view.threadId;
    let cancelled = false;
    const existingIds = new Set(events.map((event) => event.id));
    if (loadedEventsThread.current !== threadId) {
      loadedEventsThread.current = threadId;
      setEvents([]);
    }
    setEventsLoading(true);
    setEventsError(null);
    eventsApi.listAll(threadId)
      .then((loaded) => {
        if (cancelled) return;
        const loadedIds = new Set(loaded.map((event) => event.id));
        setEvents((current) => newestFirst([
          ...loaded,
          ...current.filter((event) =>
            event.threadId === threadId && !existingIds.has(event.id) && !loadedIds.has(event.id),
          ),
        ]));
      })
      .catch((error) => {
        if (!cancelled) setEventsError(toDomainError(error).message);
      })
      .finally(() => {
        if (!cancelled) setEventsLoading(false);
      });
    return () => { cancelled = true; };
  }, [view, activityRefresh]);

  // Load exclusions on mount
  useEffect(() => {
    projectsApi.defaultExclusions().then(setDefaultExclusions).catch(() => setDefaultExclusions([]));
    configApi.status().then((s) => {
      setModelEndpoint(s.endpoint);
      setModelId(s.modelId);
    }).catch(() => {});
  }, []);

  const currentThread = ("threadId" in view && view.threadId)
    ? data.threads.find((t) => t.id === view.threadId) ?? data.recordingThread
    : data.recordingThread ?? data.threads[0];

  const currentProject = currentThread
    ? data.projects.find((p) => p.id === currentThread.projectId)
    : data.projects[0];

  // Helper to switch main views
  function showView(next: PopoverMoment) {
    if (next.name === "new-thread") {
      setNewProjectId(next.projectId ?? currentProject?.id ?? data.projects[0]?.id ?? "");
      setNewThreadErrors({});
    }
    setView(next);
    if (!["threads", "settings", "new-thread", "approve-project"].includes(next.name)) {
      setLastMainView(next);
    }
  }

  // --- ACTIONS ---

  async function handleSaveMyPlace() {
    if (!currentThread) return;
    setBusy("save");
    try {
      const cp = await checkpointsApi.savePlace(currentThread.id);
      setActiveCheckpoint(cp);
      await data.refresh();
      showView({ name: "saved", threadId: currentThread.id, checkpointId: cp.id });
      toast("Checkpoint saved locally.");
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      setBusy(null);
    }
  }

  async function handlePause() {
    if (!currentThread) return;
    setBusy("pause");
    try {
      await threadsApi.pause(currentThread.id);
      await data.refresh();
      showView({ name: "paused", threadId: currentThread.id });
      toast("Thread paused. Recording stopped.");
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      setBusy(null);
    }
  }

  async function handleResume(threadId: string) {
    setBusy("resume");
    try {
      await threadsApi.resume(threadId);
      await data.refresh();
      showView({ name: "working", threadId });
      toast("Recording resumed. No command was run.");
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      setBusy(null);
    }
  }

  async function handleAddNote(event?: FormEvent) {
    if (event) event.preventDefault();
    if (!currentThread || !noteText.trim()) return;
    try {
      const added = await eventsApi.addNote(currentThread.id, {
        label: noteKind,
        text: noteText.trim(),
      });
      setEvents((current) => newestFirst([added, ...current.filter((e) => e.id !== added.id)]));
      setNoteText("");
      toast(`${noteKind.charAt(0).toUpperCase() + noteKind.slice(1)} note added.`);
    } catch (err) {
      toast(toDomainError(err).message);
    }
  }

  async function handleReviewAISharing(checkpointId: string) {
    setBusy("preview");
    setPreviewLoading(true);
    setPreview(null);
    const request = ++previewRequest.current;
    try {
      const p = await checkpointsApi.preview(checkpointId, []);
      if (request !== previewRequest.current) return;
      setPreview(p);
      setExcludedIds([]);
      showView({ name: "review", threadId: currentThread?.id ?? "", checkpointId });
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      if (request === previewRequest.current) setPreviewLoading(false);
      setBusy(null);
    }
  }

  async function toggleExcluded(eventId: EventId, checkpointId: string) {
    const nextExcluded = excludedIds.includes(eventId)
      ? excludedIds.filter((id) => id !== eventId)
      : [...excludedIds, eventId];
    setExcludedIds(nextExcluded);
    setPreviewLoading(true);
    setPreview(null);
    const request = ++previewRequest.current;
    try {
      const updated = await checkpointsApi.preview(checkpointId, nextExcluded);
      if (request === previewRequest.current) setPreview(updated);
    } catch (err) {
      if (request === previewRequest.current) toast(toDomainError(err).message);
    } finally {
      if (request === previewRequest.current) setPreviewLoading(false);
    }
  }

  async function handleGenerateDraft(checkpointId: string) {
    if (previewLoading || !preview?.canGenerate || !preview.payloadHash) return;
    setGenerating(true);
    try {
      const updated = await checkpointsApi.generate(checkpointId, excludedIds, preview.payloadHash);
      setActiveCheckpoint(updated);
      if (updated.draft?.next_action?.instruction) {
        setActionText(updated.draft.next_action.instruction);
      }
      await data.refresh();
      showView({ name: "draft", threadId: currentThread?.id ?? "", checkpointId });
      toast("AI draft summary generated.");
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      setGenerating(false);
    }
  }

  async function handleApproveSummary(checkpointId: string) {
    if (!activeCheckpoint?.draft) return;
    const capsuleToApprove = {
      ...activeCheckpoint.draft,
      next_action: {
        ...activeCheckpoint.draft.next_action,
        instruction: actionText.trim() || activeCheckpoint.draft.next_action.instruction,
      },
    };
    setBusy("approve");
    try {
      await checkpointsApi.approve(checkpointId, capsuleToApprove);
      const res = await checkpointsApi.getResume(checkpointId);
      setResumeData(res);
      await data.refresh();
      showView({ name: "return", threadId: currentThread?.id ?? "", checkpointId });
      toast("Summary approved. Ready to resume.");
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      setBusy(null);
    }
  }

  async function handleDiscardDraft(checkpointId: string) {
    try {
      await checkpointsApi.discard(checkpointId);
      await data.refresh();
      if (currentThread) {
        showView({ name: "working", threadId: currentThread.id });
      } else {
        showView({ name: "threads" });
      }
      toast("Draft discarded. Recording restored.");
    } catch (err) {
      toast(toDomainError(err).message);
    }
  }

  async function handleCopyNextAction() {
    const text = actionText || resumeData?.checkpoint.approved?.next_action?.instruction;
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      toast("Next action copied. Nothing was run.");
    } catch {
      toast("Clipboard write failed. Nothing was run.");
    }
  }

  async function handleExportJson(checkpointId: string) {
    try {
      const res = await exportsApi.json(checkpointId);
      toast(`Exported JSON to ${res.path}`);
    } catch (err) {
      toast(toDomainError(err).message);
    }
  }

  async function handleExportMarkdown(checkpointId: string) {
    try {
      const res = await exportsApi.markdown(checkpointId);
      toast(`Exported Markdown to ${res.path}`);
    } catch (err) {
      toast(toDomainError(err).message);
    }
  }

  async function handleStartThreadSubmit(e: FormEvent) {
    e.preventDefault();
    const errors: Record<string, string> = {};
    if (!newProjectId && !currentProject?.id) errors.projectId = "Choose a project folder.";
    if (!newTitle.trim()) errors.title = "Give the thread a title.";
    if (!newGoal.trim()) errors.goal = "Describe the outcome you are working toward.";
    if (Object.keys(errors).length > 0) {
      setNewThreadErrors(errors);
      return;
    }
    setNewThreadErrors({});
    setBusy("start-thread");
    try {
      const started = await threadsApi.start({
        projectId: newProjectId || currentProject!.id,
        title: newTitle.trim(),
        goal: newGoal.trim(),
        hypothesis: newHypothesis.trim() || null,
      });
      await data.refresh();
      setNewTitle("");
      setNewGoal("");
      setNewHypothesis("");
      showView({ name: "working", threadId: started.id });
      toast("Thread started and recording.");
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      setBusy(null);
    }
  }

  async function handlePickFolder() {
    setApproveErrors({});
    try {
      const folder = await chooseProjectFolder();
      if (folder) {
        inspectChosenFolder(folder);
      }
    } catch (err) {
      toast(toDomainError(err).message);
    }
  }

  function inspectChosenFolder(folder: string) {
    const request = ++projectGitCheck.current;
    setProjectPath(folder);
    setProjectName(folderName(folder));
    setProjectConsent(false);
    setProjectGitState("checking");
    setApproveErrors({});
    projectsApi.gitStatus(folder)
      .then((ready) => {
        if (request === projectGitCheck.current) {
          setProjectGitState(ready ? "ready" : "needs-init");
        }
      })
      .catch((error) => {
        if (request === projectGitCheck.current) {
          setProjectGitState("error");
          setApproveErrors({ rootPath: toDomainError(error).message });
        }
      });
  }

  async function handleChooseThreadFolder() {
    setNewThreadErrors({});
    setBusy("pick-thread-folder");
    try {
      const folder = await chooseProjectFolder();
      if (!folder) return;

      const selectedPath = folder.replace(/\/+$/, "");
      const approved = data.projects.find(
        (project) => project.canonicalRoot.replace(/\/+$/, "") === selectedPath,
      );
      if (approved) {
        setNewProjectId(approved.id);
        return;
      }

      setView({ name: "approve-project" });
      inspectChosenFolder(folder);
    } catch (err) {
      setNewThreadErrors({ projectId: toDomainError(err).message });
    } finally {
      setBusy(null);
    }
  }

  async function handleApproveProjectSubmit(e: FormEvent) {
    e.preventDefault();
    const errors: Record<string, string> = {};
    if (!projectPath) errors.rootPath = "Choose the project folder first.";
    else if (projectGitState !== "ready") errors.rootPath = "Initialize Git in this folder before approving it.";
    if (!projectName.trim()) errors.displayName = "Give the project a name.";
    if (!projectConsent) errors.consent = "Confirm that ThreadBack may collect evidence inside this folder.";
    if (Object.keys(errors).length > 0) {
      setApproveErrors(errors);
      return;
    }
    setApproveErrors({});
    setBusy("approve-project");
    try {
      const created = await projectsApi.create({
        rootPath: projectPath!,
        displayName: projectName.trim(),
      });
      await data.refresh();
      setProjectPath(null);
      setProjectName("");
      setProjectConsent(false);
      setProjectGitState("idle");
      showView({ name: "new-thread", projectId: created.id });
      toast("Project approved.");
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      setBusy(null);
    }
  }

  async function handleInitializeGit() {
    if (!projectPath) return;
    setBusy("init-git");
    setProjectGitState("initializing");
    setApproveErrors({});
    try {
      await projectsApi.initializeGit(projectPath);
      setProjectGitState("ready");
      toast("Git initialized. You can approve this project now.");
    } catch (error) {
      setProjectGitState("needs-init");
      setApproveErrors({ rootPath: toDomainError(error).message });
    } finally {
      setBusy(null);
    }
  }

  async function handleSaveModelConfig(e: FormEvent) {
    e.preventDefault();
    setBusy("save-config");
    try {
      await configApi.store({
        endpoint: modelEndpoint,
        modelId,
        timeoutSecs: 60,
        structuredOutput: "json_schema",
        apiKey: modelApiKey.trim() || null,
      });
      setModelApiKey("");
      toast("Model settings saved securely to Keychain.");
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      setBusy(null);
    }
  }

  async function handleTestConnection() {
    setBusy("test-connection");
    try {
      const res = await configApi.test({
        endpoint: modelEndpoint,
        modelId,
        timeoutSecs: 60,
        structuredOutput: "json_schema",
        apiKey: modelApiKey.trim() || null,
      });
      setTestResult(res);
      toast(res.ok ? "Connection verified." : "Connection failed.");
    } catch (err) {
      toast(toDomainError(err).message);
    } finally {
      setBusy(null);
    }
  }

  async function handleTestRedaction() {
    try {
      const res = await privacyApi.testRedaction({
        projectId: currentProject?.id ?? null,
        customPatterns: [],
        text: redactionInput,
      });
      setRedactionResult(res);
    } catch (err) {
      toast(toDomainError(err).message);
    }
  }

  const noteCount = events.filter((e) => e.payload.type === "note_added").length;

  return (
    <div className="popover" id="popover" ref={popoverRef} role="dialog" aria-label="ThreadBack menu">
      {/* Popover Header */}
      <header className="popover-header">
        <div className="popover-brand">
          <img className="pop-mark" src={evidenceListIcon} alt="" aria-hidden="true" />
          <span className="brand-copy">ThreadBack <small>Stored locally</small></span>
        </div>
        <div className="header-tools">
          <span
            className="header-status-pill"
            data-status={
              view.name === "working"
                ? "working"
                : view.name === "return" && resumeData?.drift?.hasDrift
                ? "attention"
                : ["saved", "paused", "review", "draft"].includes(view.name)
                ? "saved"
                : data.recordingThread?.state === "ACTIVE"
                ? "working"
                : "idle"
            }
          >
            <span className="dot" aria-hidden="true" />
            {view.name === "working"
              ? "Recording"
              : view.name === "paused"
              ? "Paused"
              : view.name === "return" && resumeData?.drift?.hasDrift
              ? "Drift"
              : view.name === "draft"
              ? "Draft"
              : ["saved", "review", "return"].includes(view.name)
              ? "Saved"
              : data.recordingThread?.state === "ACTIVE"
              ? "Recording"
              : "Idle"}
          </span>
          <button
            className="icon-button"
            type="button"
            onClick={() => popoverApi.close().catch(() => {})}
            aria-label="Close ThreadBack"
          >
            ×
          </button>
        </div>
      </header>

      {data.loadError && (
        <div className="backend-alert" role="alert">
          {"__TAURI_INTERNALS__" in window
            ? `Local data is unavailable: ${data.loadError}`
            : "Open the ThreadBack macOS app to create threads. The browser preview cannot access your local projects."}
        </div>
      )}

      {/* --- MOMENT 1: WORKING --- */}
      {view.name === "working" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <div className="overline">
              <span className="dot" aria-hidden="true" /> Recording this thread
            </div>
            <h2 id="view-title" tabIndex={-1}>
              {currentThread?.title || "Active thread"}
            </h2>
            <p className="description">{currentThread?.goal || "Investigating problem"}</p>
            <div className="project-line">
              <span className="project-name">{currentProject?.displayName || "Project"}</span>
              {currentThread?.startedAt && <span>· Started {formatTime(currentThread.startedAt)}</span>}
            </div>

            <div className="metric-row">
              <div className="metric">
                <strong>{eventsLoading && events.length === 0 ? "…" : events.length}</strong>
                <span>events</span>
              </div>
              <div className="metric">
                <strong>{noteCount}</strong>
                <span>notes</span>
              </div>
            </div>

            <div className="buttons">
              <button
                className="button primary"
                type="button"
                disabled={busy !== null}
                onClick={handleSaveMyPlace}
              >
                Save my place
              </button>
              <button
                className="button"
                type="button"
                disabled={busy !== null}
                onClick={handlePause}
              >
                Pause
              </button>
            </div>

            <div className="hairline" />

            <div className="section-label">Add a quick note</div>
            <form className="note-row" onSubmit={handleAddNote}>
              <select
                value={noteKind}
                onChange={(e) => setNoteKind(e.target.value as any)}
                aria-label="Note type"
              >
                <option value="fact">Fact</option>
                <option value="hypothesis">Idea</option>
                <option value="question">Question</option>
                <option value="next_step">Next step</option>
              </select>
              <input
                type="text"
                placeholder="What did you learn?"
                value={noteText}
                onChange={(e) => setNoteText(e.target.value)}
                aria-label="Quick note"
              />
              <button className="button" type="submit" aria-label="Add note">
                ↗
              </button>
            </form>

            <div className="hairline" />

            <div className="activity-heading">
              <div className="section-label">Activity · newest first</div>
              <button className="activity-refresh" type="button" onClick={() => setActivityRefresh((current) => current + 1)} disabled={eventsLoading}>
                Refresh
              </button>
            </div>
            {eventsError && <p className="form-error" role="alert">Activity could not be fully loaded: {eventsError}</p>}
            {eventsLoading && <p className="subtle" role="status">Loading activity…</p>}
            {!eventsLoading && !eventsError && events.length === 0 && (
              <p className="subtle">No activity recorded in this thread yet.</p>
            )}
            {events.length > 0 && (
              <ul className="activity" id="activity-list" aria-label="Activity, newest first">
                {events.map((evt) => {
                  const isNote = evt.payload.type === "note_added";
                  return (
                    <li key={evt.id} className={isNote ? "new-note" : undefined}>
                      <span>{activityLabel(evt)}</span>
                      <time dateTime={evt.timestamp}>{formatTime(evt.timestamp)}</time>
                    </li>
                  );
                })}
              </ul>
            )}
            <p className="subtle" style={{ marginTop: 12 }}>
              Commands appear when run through <code>tb run --</code>. File changes appear in Git snapshots, not as live updates.
            </p>
          </div>
        </section>
      )}

      {/* --- MOMENT 2: PAUSED --- */}
      {view.name === "paused" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <div className="overline saved">
              <span className="dot" aria-hidden="true" /> Recording paused
            </div>
            <h2 id="view-title" tabIndex={-1}>
              Your work is still here.
            </h2>
            <p className="description">
              The thread is paused. New commands will not be recorded until you resume it.
            </p>

            <div className="summary-card">
              <div className="section-label">Current thread</div>
              <p>
                <strong>{currentThread?.title}</strong>
                <br />
                {currentProject?.displayName} · {events.length} events recorded
              </p>
            </div>

            <div className="buttons">
              <button
                className="button primary"
                type="button"
                onClick={() => currentThread && handleResume(currentThread.id)}
              >
                Resume recording
              </button>
              <button className="button" type="button" onClick={handleSaveMyPlace}>
                Save my place
              </button>
            </div>
          </div>
        </section>
      )}

      {/* --- MOMENT 3: CHECKPOINT SAVED LOCALLY --- */}
      {view.name === "saved" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <div className="overline saved">
              <span className="dot" aria-hidden="true" /> Checkpoint saved locally
            </div>
            <h2 id="view-title" tabIndex={-1}>
              Ready when you return.
            </h2>
            <p className="description">
              Your thread, Git snapshot, and evidence are saved on this Mac.
            </p>

            <div className="summary-card">
              <div className="section-label">Saved thread</div>
              <p>
                <strong>{currentThread?.title}</strong>
                <br />
                {events.length} evidence events · {noteCount} notes
              </p>
            </div>

            <div className="notice">
              Nothing has been sent to a model. You can close ThreadBack now and still return to this checkpoint.
            </div>

            <div className="buttons">
              <button
                className="button primary"
                type="button"
                onClick={() => handleReviewAISharing(view.checkpointId)}
              >
                Review AI sharing
              </button>
              <button
                className="button"
                type="button"
                onClick={() => currentThread && handleResume(currentThread.id)}
              >
                Keep working
              </button>
            </div>

            <div className="hairline" />
            <p className="subtle">
              A model summary is optional. The saved checkpoint remains useful offline.
            </p>
          </div>
        </section>
      )}

      {/* --- MOMENT 4: REVIEW AI SHARING --- */}
      {view.name === "review" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <button
              className="back-button"
              type="button"
              onClick={() => showView({ name: "saved", threadId: view.threadId, checkpointId: view.checkpointId })}
            >
              ← Saved checkpoint
            </button>
            <div className="overline saved" style={{ marginTop: 13 }}>
              <span className="dot" aria-hidden="true" /> Nothing sent yet
            </div>
            <h2 id="view-title" tabIndex={-1}>
              Review what AI receives
            </h2>
            <p className="description">
              Choose the entries, inspect the exact payload, then decide whether to generate a draft.
            </p>

            <div className="summary-card">
              <div className="section-label">Destination</div>
              <p>
                {modelId} via {modelEndpoint}
                <br />
                <span className="subtle">Only the selected, redacted entries below</span>
              </p>
            </div>

            {previewLoading && <p className="notice" role="status">Updating the exact payload…</p>}
            {preview?.entries.map((entry) => {
              const isUnavailable = entry.disposition === "blocked" || entry.disposition === "local_only";
              const isExcluded = excludedIds.includes(entry.eventId);
              const sizeKb = (entry.sizeBytes / 1024).toFixed(1);

              return (
                <label key={entry.eventId} className="review-row" data-disabled={isUnavailable}>
                  <input
                    type="checkbox"
                    disabled={isUnavailable}
                    checked={!isUnavailable && !isExcluded}
                    onChange={() => toggleExcluded(entry.eventId, view.checkpointId)}
                  />
                  <span className="review-copy">
                    <strong>
                      {entry.kind === "git.snapshot"
                        ? "Git snapshot"
                        : entry.kind === "command.execution"
                        ? "Command output"
                        : entry.kind}
                    </strong>
                    <small>
                      {entry.disposition === "blocked"
                        ? "Credential detected; cannot be sent"
                        : entry.disposition === "local_only"
                        ? "Local only; never sent"
                        : `${sizeKb} KB · ${entry.reason ?? "Redacted evidence"}`}
                    </small>
                  </span>
                </label>
              );
            })}

            <div className="review-count" aria-live="polite">
              {preview?.counts?.included ?? 0} entries ·{" "}
              {preview ? ((preview.totalSizeBytes ?? 0) / 1024).toFixed(1) : "0.0"} KB selected
            </div>

            <details>
              <summary>Inspect exact payload and fingerprint</summary>
              <pre>{preview?.payloadJson || "No entries selected"}</pre>
              <p className="subtle" style={{ marginTop: 6 }}>
                Exact payload hash: {preview?.payloadHash ? `${preview.payloadHash.slice(0, 16)}…` : "None"}
              </p>
            </details>

            <div className="buttons review-actions">
              <button
                className="button primary"
                type="button"
                disabled={generating || previewLoading || !preview?.canGenerate}
                onClick={() => handleGenerateDraft(view.checkpointId)}
              >
                {generating ? "Generating draft…" : "Generate draft summary"}
              </button>
              <button
                className="button"
                type="button"
                onClick={() => showView({ name: "saved", threadId: view.threadId, checkpointId: view.checkpointId })}
              >
                Cancel
              </button>
            </div>

            <p className="subtle" style={{ marginTop: 10 }}>
              Generate requires a fresh, clean payload hash. Nothing has been sent yet.
            </p>
          </div>
        </section>
      )}

      {/* --- MOMENT 5: REVIEWING GENERATED DRAFT --- */}
      {view.name === "draft" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <div className="overline">
              <span className="dot" aria-hidden="true" /> Draft · your approval needed
            </div>
            <h2 id="view-title" tabIndex={-1}>
              A place to start again.
            </h2>
            <p className="description">
              Review the model’s claims and edit the next action before you approve it.
            </p>

            <div className="hairline" />

            <div className="section-label">Verified facts</div>
            {activeCheckpoint?.draft?.verified_facts && activeCheckpoint.draft.verified_facts.length > 0 ? (
              activeCheckpoint.draft.verified_facts.map((fact) => (
                <div key={fact.id} className="draft-fact">
                  {fact.claim}
                  <em>Linked to {fact.source_event_ids.length} recorded evidence sources</em>
                </div>
              ))
            ) : (
              <p className="subtle">No verified facts generated.</p>
            )}

            {activeCheckpoint?.draft?.hypotheses && activeCheckpoint.draft.hypotheses.length > 0 && (
              <>
                <div className="section-label" style={{ marginTop: 15 }}>
                  Idea to check
                </div>
                {activeCheckpoint.draft.hypotheses.map((h) => (
                  <p key={h.id} className="description">
                    {h.claim}
                  </p>
                ))}
              </>
            )}

            <div className="hairline" />

            <div className="section-label">Suggested next action</div>
            {!editingAction ? (
              <p className="next-action">
                {actionText || activeCheckpoint?.draft?.next_action?.instruction}
              </p>
            ) : (
              <textarea
                className="edit-field"
                value={actionText}
                onChange={(e) => setActionText(e.target.value)}
                aria-label="Edit next action"
              />
            )}
            <button
              className="linkish"
              type="button"
              onClick={() => setEditingAction(!editingAction)}
              style={{ marginTop: 8 }}
            >
              {editingAction ? "Save edit" : "Edit next action"}
            </button>

            <div className="buttons">
              <button
                className="button primary"
                type="button"
                disabled={busy !== null}
                onClick={() => handleApproveSummary(view.checkpointId)}
              >
                Approve summary
              </button>
              <button
                className="button"
                type="button"
                onClick={() => handleDiscardDraft(view.checkpointId)}
              >
                Discard draft
              </button>
            </div>

            <p className="subtle" style={{ marginTop: 10 }}>
              Approval stores your version separately from the model draft.
            </p>
          </div>
        </section>
      )}

      {/* --- MOMENT 6: RETURNING TO APPROVED CHECKPOINT --- */}
      {view.name === "return" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <div className={`overline ${resumeData?.drift?.hasDrift ? "attention" : "saved"}`}>
              <span className="dot" aria-hidden="true" />{" "}
              {resumeData?.drift?.hasDrift ? "Your project changed" : "Approved checkpoint"}
            </div>
            <h2 id="view-title" tabIndex={-1}>
              Pick up {currentThread?.title || "your thread"}
            </h2>
            <p className="description">
              Your approved checkpoint is ready. Check the new project state before resuming.
            </p>

            {resumeData?.drift?.hasDrift ? (
              <div className="notice warning">
                <strong>Since the checkpoint</strong>
                <br />
                {resumeData.drift.summary}
                {resumeData.drift.signals.map((sig, i) => (
                  <div key={i} style={{ marginTop: 3 }}>
                    {sig.label}: {sig.before ?? "none"} → {sig.after ?? "changed"}
                  </div>
                ))}
              </div>
            ) : (
              <div className="notice">
                Working tree matches the approved checkpoint. Ready to resume anytime.
              </div>
            )}

            <div className="summary-card">
              <div className="section-label">Your next action</div>
              <p className="next-action">
                {actionText || resumeData?.checkpoint.approved?.next_action?.instruction}
              </p>
              <button
                className="linkish"
                type="button"
                onClick={() => setShowSources(!showSources)}
                style={{ marginTop: 9 }}
                aria-expanded={showSources}
              >
                {showSources ? "Hide linked evidence" : "View linked evidence ↗"}
              </button>
              {showSources && (
                <div className="source-panel">
                  <strong>Verified Sources:</strong>
                  {resumeData?.checkpoint.approved?.verified_facts.map((f, i) => (
                    <div key={f.id} style={{ marginTop: 4 }}>
                      Source {i + 1}: {f.claim}
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div className="buttons">
              <button className="button primary" type="button" onClick={handleCopyNextAction}>
                Copy next action
              </button>
              <button
                className="button"
                type="button"
                onClick={() => currentThread && handleResume(currentThread.id)}
              >
                Resume thread
              </button>
            </div>

            <div className="buttons" style={{ marginTop: 8 }}>
              <button
                className="button ghost"
                type="button"
                onClick={() => handleExportJson(view.checkpointId)}
              >
                Export JSON
              </button>
              <button
                className="button ghost"
                type="button"
                onClick={() => handleExportMarkdown(view.checkpointId)}
              >
                Export Markdown
              </button>
            </div>

            <p className="subtle" style={{ marginTop: 12 }}>
              Nothing runs automatically. Resume starts a new evidence interval.
            </p>
          </div>
        </section>
      )}

      {/* --- MOMENT 7: COMPACT THREAD NAVIGATION --- */}
      {view.name === "threads" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <div className="overline saved">
              <span className="dot" aria-hidden="true" /> On this Mac
            </div>
            <h2 id="view-title" tabIndex={-1}>
              Your threads
            </h2>
            <p className="description">
              Jump back into an active or saved thread.
            </p>

            <div className="buttons thread-list-actions">
              <button
                className="button primary"
                type="button"
                disabled={Boolean(data.loadError)}
                onClick={() => showView({ name: "new-thread", projectId: currentProject?.id })}
              >
                Start a thread…
              </button>
            </div>

            {data.threads.length === 0 ? (
              <p className="empty">No threads yet. Start one to begin recording.</p>
            ) : (
              data.threads.map((t) => {
                const prj = data.projects.find((p) => p.id === t.projectId);
                return (
                  <button
                    key={t.id}
                    className="thread-item"
                    type="button"
                    onClick={() => {
                      if (t.state === "ACTIVE") showView({ name: "working", threadId: t.id });
                      else if (t.state === "PAUSED") showView({ name: "paused", threadId: t.id });
                      else loadLatestCheckpoint(t.id, t.state);
                    }}
                  >
                    <strong>{t.title}</strong>
                    <span>
                      {prj?.displayName ?? "Project"} · {t.state.toLowerCase()} · {formatTime(t.updatedAt)}
                    </span>
                  </button>
                );
              })
            )}

            <p className="subtle" style={{ marginTop: 9 }}>
              Project approval uses the system folder picker.
            </p>
          </div>
        </section>
      )}

      {/* --- MOMENT 8: START A THREAD --- */}
      {view.name === "new-thread" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <div className="overline saved">
              <span className="dot" aria-hidden="true" /> New thread
            </div>
            <h2 id="view-title" tabIndex={-1}>
              Start a thread
            </h2>
            <p className="description">
              Focus on one problem. Only commands in this project will be recorded.
            </p>

            <form onSubmit={handleStartThreadSubmit}>
              <div className="field">
                <label htmlFor="thread-project">Project folder</label>
                {data.projects.length > 0 && (
                  <select
                    id="thread-project"
                    value={newProjectId || currentProject?.id || ""}
                    onChange={(e) => setNewProjectId(e.target.value)}
                    disabled={busy !== null}
                  >
                    {data.projects.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.displayName} ({p.canonicalRoot})
                      </option>
                    ))}
                  </select>
                )}
                <button
                  id={data.projects.length === 0 ? "thread-project" : undefined}
                  type="button"
                  className="button choose-folder-button"
                  onClick={handleChooseThreadFolder}
                  disabled={busy !== null || Boolean(data.loadError)}
                >
                  <span aria-hidden="true">＋</span>
                  {busy === "pick-thread-folder"
                    ? "Opening folder picker…"
                    : data.projects.length > 0
                      ? "Choose another folder…"
                      : "Choose a folder…"}
                </button>
                <p className="subtle">New folders need one-time approval before recording.</p>
                {newThreadErrors.projectId && (
                  <div className="field-error">{newThreadErrors.projectId}</div>
                )}
              </div>

              <div className="field">
                <label htmlFor="thread-title">Title</label>
                <input
                  id="thread-title"
                  type="text"
                  placeholder="e.g. Duplicate capture investigation"
                  value={newTitle}
                  onChange={(e) => setNewTitle(e.target.value)}
                />
                {newThreadErrors.title && (
                  <div className="field-error">{newThreadErrors.title}</div>
                )}
              </div>

              <div className="field">
                <label htmlFor="thread-goal">Intended outcome</label>
                <input
                  id="thread-goal"
                  type="text"
                  placeholder="e.g. Find why retries produce a second provider capture"
                  value={newGoal}
                  onChange={(e) => setNewGoal(e.target.value)}
                />
                {newThreadErrors.goal && (
                  <div className="field-error">{newThreadErrors.goal}</div>
                )}
              </div>

              <div className="field">
                <label htmlFor="thread-hypothesis">Initial idea (optional)</label>
                <input
                  id="thread-hypothesis"
                  type="text"
                  placeholder="e.g. A second worker may bypass the same lock"
                  value={newHypothesis}
                  onChange={(e) => setNewHypothesis(e.target.value)}
                />
              </div>

              <div className="buttons">
                <button
                  className="button primary"
                  type="submit"
                  disabled={busy !== null || data.projects.length === 0 || Boolean(data.loadError)}
                >
                  Start recording
                </button>
                <button
                  className="button"
                  type="button"
                  onClick={() => showView({ name: "threads" })}
                >
                  Cancel
                </button>
              </div>
            </form>
          </div>
        </section>
      )}

      {/* --- MOMENT 9: PROJECT SETUP / APPROVE PROJECT --- */}
      {view.name === "approve-project" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <div className="overline saved">
              <span className="dot" aria-hidden="true" /> Project setup
            </div>
            <h2 id="view-title" tabIndex={-1}>
              Approve a project
            </h2>
            <p className="description">
              ThreadBack will only record activity inside this folder boundary.
            </p>

            <form onSubmit={handleApproveProjectSubmit}>
              <div className="field">
                <label>Folder boundary</label>
                <button
                  type="button"
                  className="button"
                  style={{ width: "100%", justifyContent: "flex-start", overflow: "hidden" }}
                  onClick={handlePickFolder}
                  disabled={busy !== null || projectGitState === "checking"}
                >
                  📁 {projectPath ? folderName(projectPath) : "Choose folder using system dialog…"}
                </button>
                {approveErrors.rootPath && (
                  <div className="field-error">{approveErrors.rootPath}</div>
                )}
                {projectPath && <span className="subtle">{projectPath}</span>}
              </div>

              {projectGitState === "checking" && (
                <p className="notice" role="status">Checking this folder for Git…</p>
              )}
              {(projectGitState === "needs-init" || projectGitState === "initializing") && (
                <div className="notice warning git-setup" role="status">
                  <strong>This folder needs Git before you can start a thread.</strong>
                  <p>Initialize Git here to track repository changes. This adds a .git folder; it does not commit your files.</p>
                  <button
                    className="button"
                    type="button"
                    disabled={busy !== null}
                    onClick={handleInitializeGit}
                  >
                    {projectGitState === "initializing" ? "Initializing Git…" : "Initialize Git in this folder"}
                  </button>
                </div>
              )}
              {projectGitState === "ready" && (
                <p className="notice" role="status">Git repository ready.</p>
              )}

              <div className="field">
                <label htmlFor="project-name">Project name</label>
                <input
                  id="project-name"
                  type="text"
                  placeholder="e.g. payments-service"
                  value={projectName}
                  onChange={(e) => setProjectName(e.target.value)}
                />
                {approveErrors.displayName && (
                  <div className="field-error">{approveErrors.displayName}</div>
                )}
              </div>

              <div className="field">
                <label>Default exclusions</label>
                <p className="subtle">
                  Secrets, credentials, `.git/`, and build artifacts are automatically ignored:
                </p>
                <p className="subtle" style={{ fontFamily: "monospace" }}>
                  {defaultExclusions.length ? defaultExclusions.join(", ") : "Loading exclusions…"}
                </p>
              </div>

              <div className="field" style={{ flexDirection: "row", alignItems: "flex-start", gap: 8, marginTop: 12 }}>
                <input
                  id="project-consent"
                  type="checkbox"
                  checked={projectConsent}
                  onChange={(e) => setProjectConsent(e.target.checked)}
                  style={{ width: "auto", minHeight: "auto", marginTop: 2 }}
                />
                <label htmlFor="project-consent" style={{ fontWeight: 400, fontSize: 12 }}>
                  Confirm ThreadBack may collect explicit evidence inside this folder.
                </label>
              </div>
              {approveErrors.consent && (
                <div className="field-error">{approveErrors.consent}</div>
              )}

              <div className="buttons">
                <button className="button primary" type="submit" disabled={busy !== null || projectGitState !== "ready"}>
                  Approve project
                </button>
                <button
                  className="button"
                  type="button"
                  onClick={() => showView({ name: "new-thread", projectId: newProjectId })}
                  disabled={busy !== null}
                >
                  Cancel
                </button>
              </div>
            </form>
          </div>
        </section>
      )}

      {/* --- MOMENT 10: PRIVACY & SETTINGS --- */}
      {view.name === "settings" && (
        <section className="view" aria-labelledby="view-title">
          <div className="view-content">
            <div className="overline saved">
              <span className="dot" aria-hidden="true" /> Local by default
            </div>
            <h2 id="view-title" tabIndex={-1}>
              Privacy &amp; settings
            </h2>
            <p className="description">
              Control what ThreadBack captures and sends.
            </p>

            <div className="settings-row">
              <span className="settings-icon">⌂</span>
              <div>
                <strong>Approved project only</strong>
                <span>
                  {currentProject?.displayName
                    ? `${currentProject.displayName} is the current project boundary.`
                    : "No project has been approved yet."}
                </span>
              </div>
            </div>

            <div className="settings-row">
              <span className="settings-icon">◎</span>
              <div>
                <strong>Explicit capture</strong>
                <span>
                  Commands appear only when run with tb run; no background shell history.
                </span>
              </div>
            </div>

            <div className="settings-row">
              <span className="settings-icon">◇</span>
              <div>
                <strong>Local checkpoints</strong>
                <span>Saving your place never contacts the model.</span>
              </div>
            </div>

            <div className="settings-row">
              <span className="settings-icon">↗</span>
              <div>
                <strong>AI requests require review</strong>
                <span>The exact selected payload is shown before each request.</span>
              </div>
            </div>

            <div className="hairline" />

            <div className="section-label">Model Connection</div>
            <form onSubmit={handleSaveModelConfig}>
              <div className="field">
                <label htmlFor="model-endpoint">Endpoint</label>
                <input
                  id="model-endpoint"
                  type="text"
                  value={modelEndpoint}
                  onChange={(e) => setModelEndpoint(e.target.value)}
                />
              </div>

              <div className="field">
                <label htmlFor="model-id">Model ID</label>
                <input
                  id="model-id"
                  type="text"
                  value={modelId}
                  onChange={(e) => setModelId(e.target.value)}
                />
              </div>

              <div className="field">
                <label htmlFor="model-api-key">API Key (stored in macOS Keychain)</label>
                <input
                  id="model-api-key"
                  type="password"
                  placeholder="Paste new API key to update…"
                  value={modelApiKey}
                  onChange={(e) => setModelApiKey(e.target.value)}
                />
              </div>

              <div className="buttons">
                <button className="button primary" type="submit" disabled={busy !== null}>
                  Save settings
                </button>
                <button
                  className="button"
                  type="button"
                  disabled={busy !== null}
                  onClick={handleTestConnection}
                >
                  Test connection
                </button>
              </div>

              {testResult && (
                <div
                  className={`notice ${testResult.ok ? "" : "warning"}`}
                  style={{ marginTop: 10 }}
                >
                  {testResult.ok ? "✓ " : "⚠ "}
                  {testResult.message}
                </div>
              )}
            </form>

            <div className="hairline" />

            <div className="section-label">Redaction Test</div>
            <div className="field">
              <input
                type="text"
                value={redactionInput}
                onChange={(e) => setRedactionInput(e.target.value)}
                aria-label="Text to test for redaction"
              />
              <button
                className="button"
                type="button"
                style={{ marginTop: 6 }}
                onClick={handleTestRedaction}
              >
                Test Redaction
              </button>
              {redactionResult && (
                <div className="notice" style={{ marginTop: 8 }}>
                  <strong>Redacted preview:</strong>
                  <br />
                  {redactionResult.redactedText}
                </div>
              )}
            </div>

            <div className="hairline" />

            <div className="section-label">Quit application</div>
            <p className="subtle">
              ThreadBack will stop running in the menu bar and pause all active recording.
            </p>
            <div className="buttons">
              <button
                className="button danger"
                type="button"
                onClick={() => popoverApi.quit().catch(() => {})}
              >
                Quit ThreadBack
              </button>
            </div>
          </div>
        </section>
      )}

      {/* Popover Footer Navigation */}
      <footer className="footer">
        <button
          className="footer-new-thread"
          type="button"
          disabled={Boolean(data.loadError)}
          onClick={() => showView({ name: "new-thread", projectId: currentProject?.id })}
          aria-current={view.name === "new-thread" ? "page" : undefined}
        >
          <span aria-hidden="true">＋</span> New thread
        </button>
        <nav aria-label="Popover navigation">
          <button
            type="button"
            id="nav-home"
            onClick={() => showView(lastMainView)}
            aria-current={!["threads", "settings", "new-thread", "approve-project"].includes(view.name) ? "page" : undefined}
          >
            Home
          </button>
          <button
            type="button"
            id="nav-threads"
            onClick={() => showView({ name: "threads" })}
            aria-current={["threads", "new-thread", "approve-project"].includes(view.name) ? "page" : undefined}
          >
            Threads
          </button>
          <button
            type="button"
            id="nav-settings"
            onClick={() => showView({ name: "settings" })}
            aria-current={view.name === "settings" ? "page" : undefined}
          >
            Privacy
          </button>
        </nav>
      </footer>

      {/* Accessible Toast Notification */}
      {toastMessage && (
        <div className="toast" role="status" aria-live="polite">
          {toastMessage}
        </div>
      )}
    </div>
  );
}
