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
const CAPSULE = {
  schema_version: "1.0",
  goal: "Find why retries create a second provider capture",
  checkpoint_summary: "The focused test still produces two provider calls, and the retry middleware change did not help.",
  verified_facts: [{ id: "fact_1", claim: "The focused retry test made two provider calls.", source_event_ids: ["evt-2"] }],
  hypotheses: [{ id: "hyp_1", claim: "A second worker bypasses the distributed lock.", confidence: "medium", source_event_ids: [] }],
  failed_attempts: [{ action: "Changed the retry middleware backoff.", outcome: "The test still failed.", learning: "Backoff does not change the call count.", source_event_ids: ["evt-2"] }],
  unresolved_questions: ["Do both workers share the same lock backend?"],
  relevant_artifacts: [{ type: "test", relative_path: "tests/payment-retry.test.mjs", reason: "Reproduces the duplicate call.", source_event_ids: [] }],
  next_action: { instruction: "Log the lock owner in both workers and rerun the focused test.", justification: "The workers may not observe the same lock state.", success_check: "Both retries observe the same lock owner.", source_event_ids: [] },
  limitations: ["The production lock backend was not inspected."],
};

export class FakeBackend {
  /** What the settings screen shows. The key is never part of it. */
  config = {
    endpoint: "https://api.tokenfactory.nebius.com/v1",
    modelId: "nvidia/nemotron-3-super-120b-a12b",
    timeoutSecs: 60,
    structuredOutput: "json_schema",
    hasApiKey: false,
    destinationHost: "api.tokenfactory.nebius.com",
  };
  /** Keys the interface sent, to check they travel only through `configuration_store`/`configuration_test`. */
  keysSent: { command: string; key: string }[] = [];
  testResult: { ok: boolean; message: string; latencyMs: number | null; modelId: string } | null = null;
  /** When true, `checkpoints_generate` waits until released or cancelled. */
  holdGenerate = false;
  private pendingGenerate: { resolve: (value: unknown) => void; reject: (error: unknown) => void } | null = null;

  thread: WorkThread;
  deletedThreads: string[] = [];
  projectsList = [
    {
      id: "project-1",
      displayName: "Payments",
      canonicalRoot: "/workspace/payments",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    },
  ];
  policy = {
    projectId: "project-1",
    exclusions: ["fixtures/sensitive/**"],
    retentionDays: 14,
    captureGit: true,
    captureCommands: true,
    captureFileMetadata: true,
    redactionPatterns: [] as string[],
  };
  drift = {
    checked: true,
    hasDrift: false,
    summary: "Repository matches the checkpoint snapshot (same branch, HEAD, and working tree).",
    signals: [] as { kind: string; label: string; before: string | null; after: string | null }[],
    checkpointSnapshot: null as any,
    currentSnapshot: null as any,
  };
  artifactFiles = new Map<string, string>([
    ["tests/payment-retry.test.mjs", "// Focused payment retry test\nassert.equal(calls, 1);\n"],
  ]);
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

  /** The capsule the fake "model" returns; tests can replace it. */
  capsule: any = CAPSULE;
  corrections = new Map<string, any[]>();

  /** The server's approval rules that the UI depends on: every source exists, and facts rest on real support. */
  private checkRules(checkpoint: Checkpoint, capsule: any) {
    const entries = (checkpoint.payload as any).entries as { eventId: string; kind: string; content: any }[];
    const byId = new Map(entries.map((entry) => [entry.eventId, entry]));
    const unproven = ["hypothesis", "question", "next_step"];
    const reject = (path: string, message: string) => {
      throw failure("INVALID_CAPSULE", `This capsule cannot be approved yet. ${path}: ${message}`, path);
    };
    capsule.verified_facts.forEach((fact: any, index: number) => {
      const path = `/verified_facts/${index}/source_event_ids`;
      if (fact.source_event_ids.length === 0) reject(path, "a verified fact needs at least one source.");
      const missing = fact.source_event_ids.find((id: string) => !byId.has(id));
      if (missing) reject(path, `${missing} is not an event in this checkpoint.`);
      const supported = fact.source_event_ids.some((id: string) => {
        const entry = byId.get(id)!;
        return entry.kind !== "note.added" || !unproven.includes(entry.content?.label);
      });
      if (!supported) reject(path, "it rests only on notes labelled hypothesis, question, or next step.");
      if (!fact.claim.trim()) reject(`/verified_facts/${index}/claim`, "the claim is empty.");
    });
    if (!capsule.next_action.instruction.trim()) reject("/next_action/instruction", "the instruction is empty.");
  }

  /** A simplified derivation: one correction per top-level section that differs. */
  private deriveCorrections(checkpoint: Checkpoint, draft: any, approved: any) {
    return Object.keys(approved)
      .filter((key) => JSON.stringify(draft[key]) !== JSON.stringify(approved[key]))
      .map((key, index) => ({
        id: `correction-${index + 1}`,
        checkpointId: checkpoint.id,
        fieldPath: `/${key}`,
        before: draft[key],
        after: approved[key],
        reason: null,
        createdAt: new Date().toISOString(),
      }));
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

  private validateConfig(input: { endpoint: string; modelId: string; timeoutSecs: number }) {
    if (!/^https:\/\//.test(input.endpoint) && !/^http:\/\/(127\.0\.0\.1|localhost)/.test(input.endpoint)) {
      throw failure("INVALID_INPUT", "The endpoint must use https.", "endpoint");
    }
    if (!input.modelId.trim()) throw failure("INVALID_INPUT", "The model ID must be 1 to 200 letters, digits, or . _ : / - characters.", "modelId");
    if (input.timeoutSecs < 5 || input.timeoutSecs > 300) throw failure("INVALID_INPUT", "The timeout must be between 5 and 300 seconds.", "timeoutSecs");
  }

  /** Lets a held generation finish successfully. */
  releaseGenerate() {
    this.pendingGenerate?.resolve(null);
    this.pendingGenerate = null;
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
        if (!this.config.hasApiKey) throw failure("NOT_CONFIGURED", "Add your model API key in Settings before generating a capsule. Nothing was sent.");
        const finish = () => {
          const checkpoint = this.checkpoints.find((candidate) => candidate.id === args.checkpointId)!;
          checkpoint.draft = JSON.parse(JSON.stringify(this.capsule));
          checkpoint.status = "generated";
          checkpoint.modelId = this.config.modelId;
          checkpoint.promptVersion = "capsule-prompt-v1";
          // A real backend returns a fresh object, not the stored one.
          return JSON.parse(JSON.stringify(checkpoint));
        };
        if (!this.holdGenerate) return finish();
        return new Promise((resolve, reject) => {
          this.pendingGenerate = { resolve: () => resolve(finish()), reject };
        });
      }
      case "checkpoints_cancel_generation":
        this.pendingGenerate?.reject(failure("CANCELLED", "Generation was cancelled. Nothing more was sent."));
        this.pendingGenerate = null;
        return null;
      case "configuration_status":
        return { ...this.config };
      case "configuration_store": {
        this.validateConfig(args.input);
        if (args.input.apiKey) {
          this.keysSent.push({ command, key: args.input.apiKey });
          this.config.hasApiKey = true;
        }
        const { apiKey: _ignored, ...settings } = args.input;
        this.config = { ...this.config, ...settings, destinationHost: new URL(settings.endpoint).hostname };
        return { ...this.config };
      }
      case "configuration_remove_key":
        this.config.hasApiKey = false;
        return { ...this.config };
      case "configuration_test": {
        this.validateConfig(args.input);
        if (args.input.apiKey) this.keysSent.push({ command, key: args.input.apiKey });
        if (this.testResult) return this.testResult;
        if (!args.input.apiKey && !this.config.hasApiKey) {
          return { ok: false, message: "Enter an API key first. Nothing was sent.", latencyMs: null, modelId: args.input.modelId };
        }
        return { ok: true, message: `Connected. ${args.input.modelId} answered in 412 ms.`, latencyMs: 412, modelId: args.input.modelId };
      }
      case "checkpoints_discard": {
        const checkpoint = this.checkpoints.find((candidate) => candidate.id === args.checkpointId)!;
        if (checkpoint.status !== "draft" && checkpoint.status !== "generated") {
          throw failure("INVALID_STATE", "Only a draft that has not been approved can be discarded.");
        }
        checkpoint.status = "discarded";
        this.add({ type: "checkpoint_discarded", payload: { checkpointId: checkpoint.id } } as any);
        this.setState("ACTIVE");
        return this.thread;
      }
      case "checkpoints_approve": {
        const checkpoint = this.checkpoints.find((candidate) => candidate.id === args.checkpointId)!;
        if (checkpoint.status === "approved") throw failure("INVALID_STATE", "This capsule was already approved.");
        if (checkpoint.status !== "generated" || !checkpoint.draft) {
          throw failure("INVALID_STATE", "There is no generated capsule to approve yet.");
        }
        const capsule = args.capsule as typeof CAPSULE;
        this.checkRules(checkpoint, capsule);
        const corrections = this.deriveCorrections(checkpoint, checkpoint.draft as any, capsule);
        checkpoint.approved = JSON.parse(JSON.stringify(capsule));
        checkpoint.status = "approved";
        checkpoint.approvedAt = new Date().toISOString();
        this.corrections.set(checkpoint.id, corrections);
        this.add({ type: "checkpoint_approved", payload: { checkpointId: checkpoint.id, correctionCount: corrections.length } } as any);
        this.setState("READY");
        return { checkpoint: JSON.parse(JSON.stringify(checkpoint)), thread: this.thread, corrections };
      }
      case "checkpoints_corrections":
        return this.corrections.get(args.checkpointId) ?? [];
      case "checkpoints_get_resume": {
        const checkpoint = this.checkpoints.find((candidate) => candidate.id === args.checkpointId) ?? this.checkpoints[0];
        if (!checkpoint) throw failure("NOT_FOUND", "That checkpoint no longer exists.");
        return {
          thread: this.thread,
          checkpoint: JSON.parse(JSON.stringify(checkpoint)),
          corrections: this.corrections.get(checkpoint.id) ?? [],
          drift: JSON.parse(JSON.stringify(this.drift)),
        };
      }
      case "checkpoints_open_artifact": {
        const checkpoint = this.checkpoints.find((candidate) => candidate.id === args.checkpointId) ?? this.checkpoints[0];
        if (!checkpoint || checkpoint.status !== "approved") {
          throw failure("INVALID_STATE", "Artifacts can only be opened from an approved capsule.");
        }
        const rel = String(args.relativePath ?? "");
        const content = this.artifactFiles.get(rel) ?? null;
        return {
          relativePath: rel,
          resolvedPath: `/workspace/payments/${rel}`,
          exists: content !== null,
          contentPreview: content,
          truncated: false,
        };
      }
      case "exports_json": {
        const checkpoint = this.checkpoints.find((candidate) => candidate.id === args.checkpointId)!;
        if (!checkpoint || checkpoint.status !== "approved" || !checkpoint.approved) {
          throw failure("INVALID_STATE", "Only an approved capsule can be exported.");
        }
        const jsonText = JSON.stringify(checkpoint.approved, null, 2);
        return {
          path: `/exports/${this.thread.id}/capsule-${checkpoint.id}.json`,
          format: "json",
          bytes: jsonText.length,
        };
      }
      case "exports_markdown": {
        const checkpoint = this.checkpoints.find((candidate) => candidate.id === args.checkpointId)!;
        if (!checkpoint || checkpoint.status !== "approved" || !checkpoint.approved) {
          throw failure("INVALID_STATE", "Only an approved capsule can be exported.");
        }
        return {
          path: `/exports/${this.thread.id}/capsule-${checkpoint.id}.md`,
          format: "markdown",
          bytes: 512,
        };
      }
      case "threads_archive":
        this.setState("ARCHIVED");
        return this.thread;
      case "threads_delete": {
        const threadId = args.threadId;
        this.deletedThreads.push(threadId);
        const eventsDeleted = this.events.length;
        const checkpointsDeleted = this.checkpoints.length;
        this.events = [];
        this.checkpoints = [];
        this.corrections.clear();
        this.setState("DELETED");
        return {
          threadId,
          eventsDeleted,
          checkpointsDeleted,
          correctionsDeleted: 0,
          artifactsDeleted: 0,
          exportsDeleted: 0,
          deletedAt: new Date().toISOString(),
        };
      }
      case "threads_list":
        return this.thread.state === "DELETED" ? [] : [this.thread];
      case "threads_pause_all":
        if (this.recording) {
          this.add({ type: "thread_paused", payload: { reason: "user" } });
          this.setState("PAUSED");
          return [this.thread];
        }
        return [];
      case "projects_list":
        return this.projectsList;
      case "projects_relink": {
        const { projectId, rootPath } = args.input;
        if (!String(rootPath ?? "").trim()) {
          throw failure("INVALID_INPUT", "New project folder is required.", "rootPath");
        }
        const project = this.projectsList.find((p) => p.id === projectId) ?? this.projectsList[0];
        if (!project) throw failure("NOT_FOUND", "That project no longer exists.");
        project.canonicalRoot = String(rootPath).trim();
        project.updatedAt = new Date().toISOString();
        this.drift = {
          checked: true,
          hasDrift: false,
          summary: "Repository matches the checkpoint snapshot (same branch, HEAD, and working tree).",
          signals: [],
          checkpointSnapshot: null,
          currentSnapshot: null,
        };
        return { ...project };
      }
      case "projects_default_exclusions":
        return [".env", ".env.*", "*.pem", "*.key", ".git/**", "node_modules/**", "target/**"];
      case "projects_get_policy":
        return { ...this.policy };
      case "projects_update_policy": {
        const next = args.policy;
        if (next.retentionDays < 1 || next.retentionDays > 365) {
          throw failure("INVALID_INPUT", "Retention must be between 1 and 365 days.", "retentionDays");
        }
        this.policy = { ...next };
        return { ...this.policy };
      }
      case "privacy_test_redaction": {
        const input = args.input;
        let text = String(input.text ?? "");
        const findings: { label: string; description: string; count: number; isSecret: boolean }[] = [];
        if (/Bearer\s+\S+/i.test(text)) {
          text = text.replace(/(Bearer\s+)\S+/gi, "$1[REDACTED:bearer-token]");
          findings.push({ label: "bearer-token", description: "a bearer token", count: 1, isSecret: true });
        }
        if (/nbf_demo_[A-Za-z0-9_]+/.test(text)) {
          text = text.replace(/nbf_demo_[A-Za-z0-9_]+/g, "[REDACTED:api-token]");
          findings.push({ label: "api-token", description: "an API token", count: 1, isSecret: true });
        }
        for (const pat of input.customPatterns ?? []) {
          if (pat && new RegExp(pat).test(text)) {
            text = text.replace(new RegExp(pat, "g"), "[REDACTED:custom]");
            findings.push({ label: "custom", description: "text matching one of your custom patterns", count: 1, isSecret: true });
          }
        }
        const secretCount = findings.filter((f) => f.isSecret).reduce((sum, f) => sum + f.count, 0);
        return {
          redactedText: text,
          findings,
          secretCount,
          hasSecret: secretCount > 0,
        };
      }
      case "privacy_expire_events":
        return { rawEventsExpired: 0 };
      default:
        throw new Error(`Unexpected command ${command}`);
    }
  };
}
