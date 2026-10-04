import { describe, expect, it } from "vitest";
import type { CheckpointEntry, ThreadEvent } from "../bindings/contracts";
import { folderName, summarizeEntry, summarizeEvent } from "./format";

const event = (payload: ThreadEvent["payload"]): ThreadEvent => ({
  id: "e1",
  threadId: "t1",
  timestamp: "2026-10-01T10:00:00Z",
  payload,
  sensitivity: "normal",
  contentHash: "h",
});

const entry = (kind: string, content: unknown): CheckpointEntry => ({
  eventId: "e1",
  timestamp: "2026-10-01T10:00:00Z",
  kind,
  content: content as CheckpointEntry["content"],
});

const excerpt = (over: Record<string, unknown> = {}) => ({
  head: "", tail: "", originalBytes: 0, omittedBytes: 0, truncated: false, binary: false, ...over,
});

describe("summarizeEvent", () => {
  it("describes a failed command in words", () => {
    const summary = summarizeEvent(
      event({
        type: "command_finished",
        payload: {
          startedEventId: "s", exitCode: 1, signal: null, durationMs: 1200,
          stdout: excerpt({ head: "2 calls", originalBytes: 7 }), stderr: excerpt(),
        },
      }),
    );
    expect(summary.text).toBe("failed (exit code 1) after 1.20 s");
    expect(summary.outputs).toEqual([{ label: "Standard output", text: "2 calls", note: undefined }]);
  });

  it("describes success and signals", () => {
    const finished = (exitCode: number | null, signal: number | null) =>
      summarizeEvent(event({ type: "command_finished", payload: { startedEventId: "s", exitCode, signal, durationMs: 50, stdout: excerpt(), stderr: excerpt() } })).text;
    expect(finished(0, null)).toBe("succeeded (exit code 0) after 0.05 s");
    expect(finished(null, 9)).toBe("ended by signal 9 after 0.05 s");
  });

  it("shows the omitted middle of truncated output and says so", () => {
    const summary = summarizeEvent(
      event({
        type: "command_finished",
        payload: {
          startedEventId: "s", exitCode: 1, signal: null, durationMs: 1,
          stdout: excerpt({ head: "start", tail: "end", originalBytes: 5_000_000, omittedBytes: 4_999_992, truncated: true }),
          stderr: excerpt({ originalBytes: 3000, binary: true }),
        },
      }),
    );
    expect(summary.outputs![0].text).toBe("start\n… 4.8 MB omitted …\nend");
    expect(summary.outputs![0].note).toMatch(/Showing the first and last parts of 4.8 MB/);
    expect(summary.outputs![1]).toEqual({ label: "Standard error", text: "", note: "2.9 KB of binary output, not stored." });
  });

  it("explains an interrupted thread without judging the user", () => {
    const text = summarizeEvent(event({ type: "thread_paused", payload: { reason: "interrupted" } })).text;
    expect(text).toMatch(/app stopped while this thread was recording/);
    expect(summarizeEvent(event({ type: "thread_paused", payload: { reason: "user" } })).text).toBe("Paused (user).");
  });

  it("keeps note labels and text exactly", () => {
    const summary = summarizeEvent(event({ type: "note_added", payload: { text: "  keep\nthis ", label: "next_step" } }));
    expect(summary).toMatchObject({ kind: "Next step", text: "  keep\nthis ", preformatted: true });
  });

  it("summarizes a git snapshot including withheld paths", () => {
    const stats = { files: [], fileCount: 2, additions: 5, deletions: 1 };
    const summary = summarizeEvent(
      event({
        type: "git_snapshot",
        payload: { branch: null, head: null, dirty: true, changedPaths: [{ path: "a", indexStatus: "M", worktreeStatus: " " }], staged: stats, unstaged: stats, excludedChanges: 3, truncated: false },
      }),
    );
    expect(summary.text).toBe("Branch detached HEAD · HEAD no commits yet · 1 changed path");
    expect(summary.detail).toContain("Staged +5 −1 in 2 files");
    expect(summary.detail).toContain("3 excluded paths not listed");
  });
});

describe("summarizeEntry", () => {
  it("renders a condensed note with its label", () => {
    expect(summarizeEntry(entry("note.added", { label: "hypothesis", text: "maybe the lock" }))).toMatchObject({
      kind: "Hypothesis", text: "maybe the lock", preformatted: true,
    });
    expect(summarizeEntry(entry("note.added", { label: "fact", text: "x", shortened: true })).detail).toBe("Shortened to fit the checkpoint.");
  });

  it("renders condensed command results and flags shortened output", () => {
    const summary = summarizeEntry(
      entry("command.finished", {
        exitCode: 2, signal: null, durationMs: 2500,
        stdout: { text: "tail of the log", originalBytes: 90_000, truncated: true },
        stderr: { originalBytes: 0 },
      }),
    );
    expect(summary.text).toBe("failed (exit code 2) after 2.50 s");
    expect(summary.outputs).toHaveLength(1);
    expect(summary.outputs![0].note).toBe("Shortened for the checkpoint; the original was 87.9 KB.");
  });

  it("renders the thread start, command start, and snapshot omissions", () => {
    expect(summarizeEntry(entry("thread.started", { goal: "Find it", hypothesis: "the lock" }))).toMatchObject({
      text: "Find it", detail: "Starting hypothesis: the lock",
    });
    expect(summarizeEntry(entry("command.started", { command: "npm test", cwd: "svc" }))).toMatchObject({
      text: "npm test", detail: "Run in svc",
    });
    const snapshot = summarizeEntry(
      entry("git.snapshot", {
        branch: "main", head: "abcdef123", dirty: true, changedPaths: [],
        staged: { files: [], fileCount: 0, additions: 0, deletions: 0 },
        unstaged: { files: [], fileCount: 1, additions: 2, deletions: 0 },
        excludedChanges: 0, truncated: false, omittedPaths: 60,
      }),
    );
    expect(snapshot.detail).toContain("60 paths left out of this checkpoint.");
  });

  it("falls back gracefully for unknown kinds", () => {
    expect(summarizeEntry(entry("file.changed", {})).kind).toBe("file changed");
  });
});

describe("folderName", () => {
  it("takes the last path segment", () => {
    expect(folderName("/Users/me/work/payments/")).toBe("payments");
    expect(folderName("C:\\work\\payments")).toBe("payments");
    expect(folderName("")).toBe("");
  });
});
