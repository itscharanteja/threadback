import { invoke } from "@tauri-apps/api/core";
import type {
  AddNoteInput,
  Checkpoint,
  ConfigurationStatus,
  ConnectionTestResult,
  ModelConfigInput,
  CheckpointId,
  CreateProjectInput,
  EventId,
  EventPage,
  PayloadPreview,
  Project,
  RecoveryReport,
  StartThreadInput,
  ThreadEvent,
  ThreadId,
  WorkThread,
} from "../bindings/contracts";

export const projects = {
  create(input: CreateProjectInput): Promise<Project> {
    return invoke<Project>("projects_create", { input });
  },

  list(): Promise<Project[]> {
    return invoke<Project[]>("projects_list");
  },

  /** Exclusions applied to every project. */
  defaultExclusions(): Promise<string[]> {
    return invoke<string[]>("projects_default_exclusions");
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
};

export const events = {
  /** Oldest first; pass the previous page's `nextCursor` to continue. */
  list(threadId: ThreadId, cursor?: string): Promise<EventPage> {
    return invoke<EventPage>("events_list", { threadId, cursor: cursor ?? null });
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
