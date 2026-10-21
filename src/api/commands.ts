import { invoke } from "@tauri-apps/api/core";
import type {
  AddNoteInput,
  ApprovalResult,
  Checkpoint,
  ConfigurationStatus,
  ConnectionTestResult,
  Correction,
  DeletionReceipt,
  FileResult,
  ModelConfigInput,
  CheckpointId,
  CreateProjectInput,
  EventId,
  EventPage,
  OpenedArtifact,
  PayloadPreview,
  Project,
  ProjectId,
  ProjectPolicy,
  RecoveryReport,
  RedactionTestInput,
  RedactionTestResult,
  RelinkProjectInput,
  ResumeCapsule,
  ResumeViewModel,
  RetentionReport,
  StartThreadInput,
  ThreadEvent,
  ThreadId,
  WorkThread,
} from "../bindings/contracts";

export const projects = {
  gitStatus(rootPath: string): Promise<boolean> {
    return invoke<boolean>("projects_git_status", { rootPath });
  },

  initializeGit(rootPath: string): Promise<void> {
    return invoke<void>("projects_initialize_git", { rootPath });
  },

  create(input: CreateProjectInput): Promise<Project> {
    return invoke<Project>("projects_create", { input });
  },

  relink(input: RelinkProjectInput): Promise<Project> {
    return invoke<Project>("projects_relink", { input });
  },

  list(): Promise<Project[]> {
    return invoke<Project[]>("projects_list");
  },

  /** Exclusions applied to every project. */
  defaultExclusions(): Promise<string[]> {
    return invoke<string[]>("projects_default_exclusions");
  },

  getPolicy(projectId: ProjectId): Promise<ProjectPolicy> {
    return invoke<ProjectPolicy>("projects_get_policy", { projectId });
  },

  updatePolicy(policy: ProjectPolicy): Promise<ProjectPolicy> {
    return invoke<ProjectPolicy>("projects_update_policy", { policy });
  },
};

export const recovery = {
  /** Threads that were still recording when the app last stopped. */
  reports(): Promise<RecoveryReport[]> {
    return invoke<RecoveryReport[]>("recovery_reports");
  },
};

export const threads = {
  list(): Promise<WorkThread[]> {
    return invoke<WorkThread[]>("threads_list");
  },

  /** Creates the thread and records its first Git snapshot. */
  start(input: StartThreadInput): Promise<WorkThread> {
    return invoke<WorkThread>("threads_start", { input });
  },

  pause(threadId: ThreadId): Promise<WorkThread> {
    return invoke<WorkThread>("threads_pause", { threadId });
  },

  pauseAll(): Promise<WorkThread[]> {
    return invoke<WorkThread[]>("threads_pause_all");
  },

  resume(threadId: ThreadId): Promise<WorkThread> {
    return invoke<WorkThread>("threads_resume", { threadId });
  },

  /** Ends the thread and saves its checkpoint draft. */
  end(threadId: ThreadId): Promise<WorkThread> {
    return invoke<WorkThread>("threads_end", { threadId });
  },

  archive(threadId: ThreadId): Promise<WorkThread> {
    return invoke<WorkThread>("threads_archive", { threadId });
  },

  delete(threadId: ThreadId): Promise<DeletionReceipt> {
    return invoke<DeletionReceipt>("threads_delete", { threadId });
  },
};

export const events = {
  /** Oldest first; pass the previous page's `nextCursor` to continue. */
  list(threadId: ThreadId, cursor?: string): Promise<EventPage> {
    return invoke<EventPage>("events_list", { threadId, cursor: cursor ?? null });
  },

  /** Read the complete timeline, following the native API's 100-event pages. */
  async listAll(threadId: ThreadId): Promise<ThreadEvent[]> {
    const all: ThreadEvent[] = [];
    const seenCursors = new Set<string>();
    let cursor: string | undefined;
    do {
      const page = await events.list(threadId, cursor);
      all.push(...page.events);
      if (!page.nextCursor) break;
      if (seenCursors.has(page.nextCursor)) {
        throw new Error("Activity pagination repeated a cursor.");
      }
      seenCursors.add(page.nextCursor);
      cursor = page.nextCursor;
    } while (cursor);
    return all;
  },

  addNote(threadId: ThreadId, note: AddNoteInput): Promise<ThreadEvent> {
    return invoke<ThreadEvent>("events_add_note", { threadId, note });
  },

  captureGit(threadId: ThreadId): Promise<ThreadEvent> {
    return invoke<ThreadEvent>("events_capture_git", { threadId });
  },
};

export const checkpoints = {
  /** Save My Place: works offline and stores a local draft. */
  savePlace(threadId: ThreadId): Promise<Checkpoint> {
    return invoke<Checkpoint>("checkpoints_save_place", { threadId });
  },

  /** Newest first. */
  list(threadId: ThreadId): Promise<Checkpoint[]> {
    return invoke<Checkpoint[]>("checkpoints_list", { threadId });
  },

  get(checkpointId: CheckpointId): Promise<Checkpoint> {
    return invoke<Checkpoint>("checkpoints_get", { checkpointId });
  },

  /** Return view model with live repository drift comparison. */
  getResume(checkpointId: CheckpointId): Promise<ResumeViewModel> {
    return invoke<ResumeViewModel>("checkpoints_get_resume", { checkpointId });
  },

  /** Explicitly opens a project-relative artifact after a user click. */
  openArtifact(checkpointId: CheckpointId, relativePath: string): Promise<OpenedArtifact> {
    return invoke<OpenedArtifact>("checkpoints_open_artifact", { checkpointId, relativePath });
  },

  /**
   * Asks the model for a capsule from the reviewed payload and resolves to the
   * checkpoint with its draft. Refused if the payload is blocked or has changed.
   */
  generate(checkpointId: CheckpointId, excludedEventIds: EventId[], reviewedHash: string): Promise<Checkpoint> {
    return invoke<Checkpoint>("checkpoints_generate", { checkpointId, excludedEventIds, reviewedHash });
  },

  /** Stops the generation in progress. Nothing more is sent. */
  cancelGeneration(): Promise<void> {
    return invoke<void>("checkpoints_cancel_generation");
  },

  /** Throws a draft away and returns the thread to recording. */
  discard(checkpointId: CheckpointId): Promise<WorkThread> {
    return invoke<WorkThread>("checkpoints_discard", { checkpointId });
  },

  /** What would be sent, with the excluded entries left out. Sends nothing. */
  preview(checkpointId: CheckpointId, excludedEventIds: EventId[] = []): Promise<PayloadPreview> {
    return invoke<PayloadPreview>("checkpoints_preview", { checkpointId, excludedEventIds });
  },

  /**
   * Approves the user's final capsule (the draft with their edits). The server
   * re-checks it and derives the corrections itself; the draft is never changed.
   */
  approve(checkpointId: CheckpointId, capsule: ResumeCapsule): Promise<ApprovalResult> {
    return invoke<ApprovalResult>("checkpoints_approve", { checkpointId, capsule });
  },

  /** The changes made when the checkpoint was approved, oldest first. */
  corrections(checkpointId: CheckpointId): Promise<Correction[]> {
    return invoke<Correction[]>("checkpoints_corrections", { checkpointId });
  },
};

export const exports = {
  json(checkpointId: CheckpointId): Promise<FileResult> {
    return invoke<FileResult>("exports_json", { checkpointId });
  },

  markdown(checkpointId: CheckpointId): Promise<FileResult> {
    return invoke<FileResult>("exports_markdown", { checkpointId });
  },
};

export const privacy = {
  testRedaction(input: RedactionTestInput): Promise<RedactionTestResult> {
    return invoke<RedactionTestResult>("privacy_test_redaction", { input });
  },

  expireEvents(): Promise<RetentionReport> {
    return invoke<RetentionReport>("privacy_expire_events");
  },
};

export const configuration = {
  status(): Promise<ConfigurationStatus> {
    return invoke<ConfigurationStatus>("configuration_status");
  },

  /** The key, if given, goes straight to the Keychain. Nothing returned contains it. */
  store(input: ModelConfigInput): Promise<ConfigurationStatus> {
    return invoke<ConfigurationStatus>("configuration_store", { input });
  },

  removeApiKey(): Promise<ConfigurationStatus> {
    return invoke<ConfigurationStatus>("configuration_remove_key");
  },

  /** One tiny request that contains none of the user's evidence. Saves nothing. */
  test(input: ModelConfigInput): Promise<ConnectionTestResult> {
    return invoke<ConnectionTestResult>("configuration_test", { input });
  },
};

export const popover = {
  close(): Promise<void> {
    return invoke<void>("popover_close");
  },
  setStatus(status: "working" | "saved" | "attention" | "idle"): Promise<void> {
    return invoke<void>("popover_set_status", { status });
  },
  beginDialog(): Promise<void> {
    return invoke<void>("popover_begin_dialog");
  },
  endDialog(): Promise<void> {
    return invoke<void>("popover_end_dialog");
  },
  quit(): Promise<void> {
    return invoke<void>("app_quit");
  },
};
