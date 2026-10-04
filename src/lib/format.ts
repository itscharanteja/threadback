import type {
  CheckpointEntry,
  CheckpointTrigger,
  CommandFinishedPayload,
  CommandStartedPayload,
  GitSnapshot,
  NoteLabel,
  OutputExcerpt,
  ThreadEvent,
  ThreadState,
} from "../bindings/contracts";

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
  /** Captured output, shown expandable. */
  outputs?: { label: string; text: string; note?: string }[];
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
      return { kind: "Paused", text: describePause(entry.payload.reason) };
    case "thread_resumed":
      return { kind: "Resumed", text: "Recording resumed." };
    case "git_snapshot":
      return summarizeSnapshot(entry.payload as unknown as GitSnapshot);
    case "command_started": {
      const started = entry.payload as unknown as CommandStartedPayload;
      return { kind: "Command", text: started.command, detail: `Run in ${started.cwd}` };
    }
    case "command_finished":
      return summarizeCommandFinished(entry.payload as unknown as CommandFinishedPayload);
    case "note_added":
      return { kind: NOTE_LABELS[entry.payload.label], text: entry.payload.text, preformatted: true };
    default:
      return { kind: entry.type.replace(/_/g, " "), text: "Recorded." };
  }
}

export function describePause(reason: string): string {
  return reason === "interrupted"
    ? "The app stopped while this thread was recording. Nothing was captured after the previous event until it was reopened."
    : `Paused (${reason}).`;
}

function describeExit(finished: { exitCode: number | null; signal: number | null }): string {
  if (finished.exitCode !== null) {
    return finished.exitCode === 0 ? "succeeded (exit code 0)" : `failed (exit code ${finished.exitCode})`;
  }
  return `ended by signal ${finished.signal ?? "unknown"}`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function excerptOutput(label: string, excerpt: OutputExcerpt) {
  if (excerpt.originalBytes === 0) return null;
  if (excerpt.binary) {
    return { label, text: "", note: `${formatBytes(excerpt.originalBytes)} of binary output, not stored.` };
  }
  const note = excerpt.truncated
    ? `Showing the first and last parts of ${formatBytes(excerpt.originalBytes)}; ${formatBytes(excerpt.omittedBytes)} omitted.`
    : undefined;
  const text = excerpt.truncated
    ? `${excerpt.head}\n… ${formatBytes(excerpt.omittedBytes)} omitted …\n${excerpt.tail}`
    : excerpt.head + excerpt.tail;
  return { label, text, note };
}

function summarizeCommandFinished(finished: CommandFinishedPayload): EventSummary {
  const seconds = (finished.durationMs / 1000).toFixed(finished.durationMs < 10_000 ? 2 : 1);
  const outputs = [
    excerptOutput("Standard output", finished.stdout),
    excerptOutput("Standard error", finished.stderr),
  ].filter((output): output is NonNullable<typeof output> => output !== null);
  return {
    kind: "Command finished",
    text: `${describeExit(finished)} after ${seconds} s`,
    outputs,
  };
}

export const TRIGGER_LABELS: Record<CheckpointTrigger, string> = {
  manual: "Save My Place",
  idle_suggestion: "Inactivity suggestion",
  thread_ended: "Thread ended",
};

interface CondensedOutput {
  text?: string;
  originalBytes: number;
  truncated?: boolean;
  binary?: boolean;
}

function condensedOutput(label: string, output: CondensedOutput | undefined) {
  if (!output || output.originalBytes === 0) return null;
  if (output.binary) {
    return { label, text: "", note: `${formatBytes(output.originalBytes)} of binary output, not stored.` };
  }
  return {
    label,
    text: output.text ?? "",
    note: output.truncated
      ? `Shortened for the checkpoint; the original was ${formatBytes(output.originalBytes)}.`
      : undefined,
  };
}

/** Describes one condensed checkpoint entry. Its content is shaped differently from a raw event. */
export function summarizeEntry(entry: CheckpointEntry): EventSummary {
  const content = entry.content as Record<string, any>;
  switch (entry.kind) {
    case "thread.started":
      return {
        kind: "Thread started",
        text: String(content.goal ?? ""),
        detail: content.hypothesis ? `Starting hypothesis: ${content.hypothesis}` : undefined,
      };
    case "thread.paused":
      return { kind: "Paused", text: describePause(String(content.reason ?? "")) };
    case "thread.resumed":
      return { kind: "Resumed", text: "Recording resumed." };
    case "note.added":
      return {
        kind: NOTE_LABELS[content.label as NoteLabel] ?? "Note",
        text: String(content.text ?? ""),
        detail: content.shortened ? "Shortened to fit the checkpoint." : undefined,
        preformatted: true,
      };
    case "git.snapshot": {
      const summary = summarizeSnapshot(content as unknown as GitSnapshot);
      const omitted = Number(content.omittedPaths ?? 0);
      return omitted > 0
        ? { ...summary, detail: `${summary.detail ?? ""} ${pluralize(omitted, "path")} left out of this checkpoint.`.trim() }
        : summary;
    }
    case "command.started":
      return { kind: "Command", text: String(content.command ?? ""), detail: `Run in ${content.cwd}` };
    case "command.finished": {
      const seconds = (Number(content.durationMs ?? 0) / 1000).toFixed(2);
      const outputs = [
        condensedOutput("Standard output", content.stdout),
        condensedOutput("Standard error", content.stderr),
      ].filter((output): output is NonNullable<typeof output> => output !== null);
      return {
        kind: "Command finished",
        text: `${describeExit({ exitCode: content.exitCode ?? null, signal: content.signal ?? null })} after ${seconds} s`,
        outputs,
      };
    }
    default:
      return { kind: entry.kind.replace(/[._]/g, " "), text: "Recorded." };
  }
}

export interface TimelineItem {
  id: string;
  timestamp: string;
  summary: EventSummary;
}

export const eventItems = (events: ThreadEvent[]): TimelineItem[] =>
  events.map((event) => ({ id: event.id, timestamp: event.timestamp, summary: summarizeEvent(event) }));

export const entryItems = (entries: CheckpointEntry[]): TimelineItem[] =>
  entries.map((entry) => ({ id: entry.eventId, timestamp: entry.timestamp, summary: summarizeEntry(entry) }));

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
