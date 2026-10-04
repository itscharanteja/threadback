import type {
  Checkpoint,
  CheckpointPayload,
  EventPayload,
  ThreadEvent,
  ThreadState,
  WorkThread,
} from "../bindings/contracts";

export interface FakeError {
  code: string;
  message: string;
  retryable: boolean;
  field: string | null;
}

const failure = (code: string, message: string, field: string | null = null): FakeError => ({
  code,
  message,
  retryable: false,
  field,
});

/**
 * An in-memory stand-in for the Rust commands, with the same recording rules
 * the UI depends on: notes and snapshots only while recording, and failures
 * rejected as the serialized error envelope.
 */
export class FakeBackend {
  thread: WorkThread;
  events: ThreadEvent[] = [];
  checkpoints: Checkpoint[] = [];
  calls: { command: string; args: any }[] = [];
  private failures = new Map<string, FakeError>();
  private counter = 0;

  constructor(state: ThreadState = "ACTIVE") {
    const now = new Date().toISOString();
    this.thread = {
      id: "thread-1",
      projectId: "project-1",
      title: "Duplicate capture investigation",
      goal: "Find why retries create a second provider capture",
      state,
      startedAt: now,
      endedAt: null,
      createdAt: now,
      updatedAt: now,
    };
    this.add({ type: "thread_started", payload: { goal: this.thread.goal, hypothesis: null } });
  }

  get recording() {
    return this.thread.state === "ACTIVE" || this.thread.state === "RESUMED";
  }

  failNext(command: string, message: string, field: string | null = null, code = "INVALID_STATE") {
    this.failures.set(command, failure(code, message, field));
  }

  /** Adds an entry to the newest checkpoint draft, such as a kind that must stay local. */
  addCheckpointEntry(entry: { eventId: string; kind: string; content: Record<string, unknown> }) {
    const payload = this.checkpoints[0].payload as any;
    payload.entries.push({ ...entry, timestamp: new Date().toISOString() });
  }

  private previewFor(checkpointId: string, excluded: string[]) {
    const checkpoint = this.checkpoints.find((candidate) => candidate.id === checkpointId)!;
    const payload = checkpoint.payload as any;
    const sendable = ["thread.started", "note.added", "git.snapshot", "command.started", "command.finished"];
    const entries = payload.entries.map((entry: any) => {
      let disposition = "included";
      let reason: string | null = null;
      let value = entry.content;
      const text = String(entry.content?.text ?? "");
      if (!sendable.includes(entry.kind)) {
        disposition = "local_only";
        reason = "This kind of evidence is never sent.";
      } else if (excluded.includes(entry.eventId)) {
        disposition = "excluded";
        reason = "Excluded by you.";
      } else if (text.includes("ACME-")) {
        disposition = "blocked";
        reason = "Contains text matching one of your custom patterns that would be sent. Exclude this entry to continue.";
        value = { ...entry.content, text: text.replace(/ACME-\d+/g, "[REDACTED:custom]") };
      }
      return {
        eventId: entry.eventId,
        timestamp: entry.timestamp,
        kind: entry.kind,
        disposition,
        value,
        sizeBytes: JSON.stringify(value).length,
        reason,
        findings: [],
      };
    });
    const included = entries.filter((entry: any) => entry.disposition === "included");
    const count = (name: string) => entries.filter((entry: any) => entry.disposition === name).length;
    const payloadJson = JSON.stringify({ goal: payload.goal, entries: included.map((e: any) => ({ eventId: e.eventId, content: e.value })) });
    return {
      checkpointId,
      threadTitle: payload.threadTitle,
      goal: payload.goal,
      trigger: payload.trigger,
      finalSnapshot: payload.finalSnapshot,
      entries,
      counts: { included: included.length, excluded: count("excluded"), blocked: count("blocked"), localOnly: count("local_only") },
      notices: [],
      totalSizeBytes: payloadJson.length,
      payloadJson,
      payloadHash: `hash:${included.map((e: any) => e.eventId).join("+")}`.padEnd(64, "0"),
      hasBlockingFindings: count("blocked") > 0,
      canGenerate: count("blocked") === 0 && included.length > 0,
    };
  }

  callsTo(command: string) {
    return this.calls.filter((call) => call.command === command);
  }

  /** Something recorded outside the UI, such as a command run through `tb`. */
  addExternalCommand(command: string, exitCode: number) {
    const started = this.add({ type: "command_started", payload: { command, cwd: "." } });
    this.add({
      type: "command_finished",
      payload: {
        startedEventId: started.id,
        exitCode,
        signal: null,
        durationMs: 1200,
        stdout: { head: "2 provider calls\n", tail: "", originalBytes: 17, omittedBytes: 0, truncated: false, binary: false, redactions: 0 },
        stderr: { head: "", tail: "", originalBytes: 0, omittedBytes: 0, truncated: false, binary: false, redactions: 0 },
      },
    });
  }

  private add(payload: EventPayload): ThreadEvent {
    this.counter += 1;
    const event: ThreadEvent = {
      id: `evt-${this.counter}`,
      threadId: this.thread.id,
      timestamp: new Date(Date.now() + this.counter).toISOString(),
      payload,
      sensitivity: "normal",
      contentHash: "hash",
    };
    this.events.push(event);
    return event;
  }

  private setState(state: ThreadState) {
    this.thread = { ...this.thread, state, updatedAt: new Date(Date.now() + this.counter).toISOString() };
  }

  private requireRecording() {
    if (!this.recording) throw failure("INVALID_STATE", "This thread is not active, so it is not recording evidence.");
  }

  /** Save My Place needs a recording thread; ending also works from a pause, as in the Rust service. */
  private saveCheckpoint(trigger: CheckpointPayload["trigger"]): Checkpoint {
    if (trigger === "thread_ended" && this.thread.state === "PAUSED") {
      // allowed
    } else {
      this.requireRecording();
    }
    const interval = [...this.events];
    const payload: CheckpointPayload = {
      version: 1,
      thread_title: this.thread.title,
      threadTitle: this.thread.title,
      goal: this.thread.goal,
      trigger,
      startEventId: interval[0].id,
      endEventId: interval[interval.length - 1].id,
      intervalEventCount: interval.length,
      finalSnapshot: { captured: false, eventId: null, reason: "The folder is not inside a Git repository." },
      entries: interval
        .filter((event) => event.payload.type === "note_added" || event.payload.type === "thread_started")
        .map((event) => ({
          eventId: event.id,
          timestamp: event.timestamp,
          kind: event.payload.type === "note_added" ? "note.added" : "thread.started",
          content: event.payload.payload as any,
        })),
      omissions: { duplicatesRemoved: 0, overBudgetRemoved: 0, notesShortened: 0, unreadable: 0 },
      totalChars: 100,
    } as unknown as CheckpointPayload;
    const checkpoint: Checkpoint = {
      id: `checkpoint-${this.checkpoints.length + 1}`,
      threadId: this.thread.id,
      startEventId: payload.startEventId,
      endEventId: payload.endEventId,
      status: "draft",
      modelId: null,
      promptVersion: null,
      payload: payload as any,
      draft: null,
      approved: null,
      createdAt: new Date().toISOString(),
      approvedAt: null,
    };
    this.checkpoints.unshift(checkpoint);
    this.setState("CHECKPOINT_DRAFT");
    return checkpoint;
  }

  invoke = async (command: string, args: any = {}): Promise<unknown> => {
    this.calls.push({ command, args });
    const injected = this.failures.get(command);
    if (injected) {
      this.failures.delete(command);
      throw injected;
    }
    switch (command) {
      case "events_list":
        return { events: this.events, nextCursor: null };
      case "events_add_note": {
        this.requireRecording();
        if (!args.note.text.trim()) throw failure("INVALID_INPUT", "A note cannot be empty.", "text");
        return this.add({ type: "note_added", payload: { text: args.note.text, label: args.note.label } });
      }
      case "events_capture_git": {
        this.requireRecording();
        return this.add({
          type: "git_snapshot",
          payload: {
            branch: "main", head: "1234567890abcdef", dirty: false, changedPaths: [],
            staged: { files: [], fileCount: 0, additions: 0, deletions: 0 },
            unstaged: { files: [], fileCount: 0, additions: 0, deletions: 0 },
            excludedChanges: 0, truncated: false,
          },
        });
      }
      case "threads_pause":
        this.requireRecording();
        this.add({ type: "thread_paused", payload: { reason: "user" } });
        this.setState("PAUSED");
        return this.thread;
      case "threads_resume":
        this.add({ type: "thread_resumed", payload: { prior_checkpoint_id: null } });
        this.setState("ACTIVE");
        return this.thread;
      case "threads_end":
        this.saveCheckpoint("thread_ended");
        return this.thread;
      case "checkpoints_save_place":
        return this.saveCheckpoint("manual");
      case "checkpoints_list":
        return this.checkpoints;
      case "checkpoints_preview":
        return this.previewFor(args.checkpointId, args.excludedEventIds ?? []);
      case "checkpoints_generate": {
        const preview = this.previewFor(args.checkpointId, args.excludedEventIds ?? []);
        if (preview.hasBlockingFindings) throw failure("PAYLOAD_BLOCKED", "Something in this payload contains a credential.");
        if (preview.counts.included === 0) throw failure("PAYLOAD_BLOCKED", "Nothing is selected to send. Include at least one entry.");
        if (preview.payloadHash !== args.reviewedHash) throw failure("PAYLOAD_CHANGED", "What would be sent has changed since you reviewed it. Review it again.");
        throw failure("NOT_CONFIGURED", "The model connection is not set up yet. Nothing was sent.");
      }
      case "checkpoints_discard": {
        const checkpoint = this.checkpoints.find((candidate) => candidate.id === args.checkpointId)!;
        if (checkpoint.status !== "draft") throw failure("INVALID_STATE", "Only a draft that has not been used can be discarded.");
        checkpoint.status = "discarded";
        this.add({ type: "checkpoint_discarded", payload: { checkpointId: checkpoint.id } } as any);
        this.setState("ACTIVE");
        return this.thread;
      }
      default:
        throw new Error(`Unexpected command ${command}`);
    }
  };
}
