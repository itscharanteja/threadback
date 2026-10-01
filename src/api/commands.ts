import { invoke } from "@tauri-apps/api/core";
import type {
  AddNoteInput,
  CreateProjectInput,
  EventPage,
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
