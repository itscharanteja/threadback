import type { GitSnapshot, NoteLabel, ThreadEvent, ThreadState } from "../bindings/contracts";

export function formatTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit",
  });
}

export const NOTE_LABELS: Record<NoteLabel, string> = {
  fact: "Fact",
  hypothesis: "Hypothesis",
  question: "Question",
  next_step: "Next step",
};

/** Every state is described in words and with a glyph, never by colour alone. */
export const STATE_DISPLAY: Record<ThreadState, { glyph: string; label: string }> = {
  ACTIVE: { glyph: "●", label: "Active — recording evidence" },
  RESUMED: { glyph: "●", label: "Resumed — recording evidence" },
  PAUSED: { glyph: "⏸", label: "Paused — not recording" },
  CHECKPOINT_DRAFT: { glyph: "✎", label: "Checkpoint draft — not recording" },
  READY: { glyph: "✓", label: "Ready to resume" },
  FAILED: { glyph: "!", label: "Checkpoint needs attention" },
  ARCHIVED: { glyph: "▪", label: "Archived" },
  DELETED: { glyph: "×", label: "Deleted" },
};

export function isRecording(state: ThreadState): boolean {
  return state === "ACTIVE" || state === "RESUMED";
}

export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

export interface EventSummary {
  kind: string;
  text: string;
  detail?: string;
  /** Preserves line breaks, used for user-authored text. */
  preformatted?: boolean;
}

export function summarizeEvent(event: ThreadEvent): EventSummary {
  const entry = event.payload;
  switch (entry.type) {
    case "thread_started":
      return {
        kind: "Thread started",
        text: entry.payload.goal,
        detail: entry.payload.hypothesis ? `Starting hypothesis: ${entry.payload.hypothesis}` : undefined,
      };
    case "thread_paused":
      return {
        kind: "Paused",
        text:
          entry.payload.reason === "interrupted"
            ? "The app stopped while this thread was recording. Nothing was captured after the previous event until it was reopened."
            : `Paused (${entry.payload.reason}).`,
      };
    case "thread_resumed":
      return { kind: "Resumed", text: "Recording resumed." };
    case "git_snapshot":
      return summarizeSnapshot(entry.payload as unknown as GitSnapshot);
    case "note_added":
      return { kind: NOTE_LABELS[entry.payload.label], text: entry.payload.text, preformatted: true };
    default:
      return { kind: entry.type.replace(/_/g, " "), text: "Recorded." };
  }
}

function summarizeSnapshot(snapshot: GitSnapshot): EventSummary {
  const branch = snapshot.branch ?? "detached HEAD";
  const head = snapshot.head ? snapshot.head.slice(0, 7) : "no commits yet";
  const changes = snapshot.dirty
    ? `${pluralize(snapshot.changedPaths.length, "changed path")}`
    : "working tree clean";
  const lines = (stats: GitSnapshot["staged"]) => `+${stats.additions} −${stats.deletions}`;
  const detail = snapshot.dirty
    ? `Staged ${lines(snapshot.staged)} in ${pluralize(snapshot.staged.fileCount, "file")}; ` +
      `unstaged ${lines(snapshot.unstaged)} in ${pluralize(snapshot.unstaged.fileCount, "file")}.` +
      (snapshot.excludedChanges > 0
        ? ` ${pluralize(snapshot.excludedChanges, "excluded path")} not listed.`
        : "")
    : undefined;
  return {
    kind: "Git snapshot",
    text: `Branch ${branch} · HEAD ${head} · ${changes}`,
    detail,
  };
}

/** Folder name of a path, used to suggest a project name. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? "";
}
