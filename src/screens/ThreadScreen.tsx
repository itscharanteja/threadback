import { useCallback, useEffect, useRef, useState } from "react";
import type { ApprovalResult, Checkpoint, Correction, Project, ThreadEvent, WorkThread } from "../bindings/contracts";
import { checkpoints as checkpointsApi, events as eventsApi, threads as threadsApi } from "../api/commands";
import { toDomainError } from "../api/errors";
import { ApprovedCapsule } from "../components/ApprovedCapsule";
import { CapsuleReview } from "../components/CapsuleReview";
import { CheckpointDraftView } from "../components/CheckpointDraftView";
import { NoteForm } from "../components/NoteForm";
import { PayloadPreviewPanel } from "../components/PayloadPreviewPanel";
import { Timeline } from "../components/Timeline";
import { STATE_DISPLAY, eventItems, formatTime, isRecording, pluralize } from "../lib/format";

const MAX_PAGES = 50;
export const LIVE_REFRESH_MS = 2000;

interface ThreadScreenProps {
  thread: WorkThread;
  project: Project | undefined;
  /** Called after any action so the rest of the app sees the new state. */
  onChanged: () => Promise<void> | void;
  /** Opens the model settings, for a generation that needs a key. */
  onOpenSettings?: () => void;
  /** Called after the thread has been permanently deleted. */
  onDeleted?: () => void;
  /** How often the evidence refreshes while the thread is recording. */
  refreshMs?: number;
}

const sameEvents = (a: ThreadEvent[] | null, b: ThreadEvent[]) =>
  a !== null && a.length === b.length && a.every((event, index) => event.id === b[index].id);

export function ThreadScreen({
  thread,
  project,
  onChanged,
  onOpenSettings = () => {},
  onDeleted = () => {},
  refreshMs = LIVE_REFRESH_MS,
}: ThreadScreenProps) {
  const [events, setEvents] = useState<ThreadEvent[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [checkpoint, setCheckpoint] = useState<Checkpoint | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const [confirmingEnd, setConfirmingEnd] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const [corrections, setCorrections] = useState<Correction[] | undefined>(undefined);
  const draftHeading = useRef<HTMLHeadingElement>(null);
  const capsuleHeading = useRef<HTMLHeadingElement>(null);
  const previewHeading = useRef<HTMLHeadingElement>(null);
  const confirmHeading = useRef<HTMLHeadingElement>(null);
  const deleteHeading = useRef<HTMLHeadingElement>(null);
  const focusDraftNext = useRef(false);
  const focusCapsuleNext = useRef(false);
  const focusPreviewNext = useRef(false);

  const recording = isRecording(thread.state);
  const showsDraft = ["CHECKPOINT_DRAFT", "FAILED", "READY", "ARCHIVED"].includes(thread.state);
  const canArchive = ["PAUSED", "READY", "FAILED"].includes(thread.state);

  const loadEvents = useCallback(async () => {
    try {
      const all: ThreadEvent[] = [];
      let cursor: string | undefined;
      for (let page = 0; page < MAX_PAGES; page += 1) {
        const result = await eventsApi.list(thread.id, cursor);
        all.push(...result.events);
        if (!result.nextCursor) break;
        cursor = result.nextCursor;
      }
      setEvents((previous) => (sameEvents(previous, all) ? previous : all));
      setLoadError(null);
    } catch (failure) {
      setLoadError(toDomainError(failure).message);
    }
  }, [thread.id]);

  useEffect(() => {
    setEvents(null);
    setCheckpoint(null);
    setCorrections(undefined);
    setConfirmingEnd(false);
    setConfirmingDelete(false);
    void loadEvents();
  }, [loadEvents]);

  // Commands run through `tb` arrive while this screen is open.
  useEffect(() => {
    if (!recording) return;
    const timer = window.setInterval(() => {
      if (!document.hidden) void loadEvents();
    }, refreshMs);
    return () => window.clearInterval(timer);
  }, [recording, loadEvents, refreshMs]);

  useEffect(() => {
    if (!showsDraft) {
      setCheckpoint(null);
      return;
    }
    checkpointsApi
      .list(thread.id)
      .then((list) => setCheckpoint(list.find((candidate) => candidate.status !== "discarded") ?? null))
      .catch((failure) => setLoadError(toDomainError(failure).message));
  }, [showsDraft, thread.id, thread.updatedAt]);

  useEffect(() => {
    if (checkpoint?.draft && focusCapsuleNext.current) {
      focusCapsuleNext.current = false;
      capsuleHeading.current?.focus();
    }
    // Closing the review is what puts the capsule on screen.
  }, [checkpoint, reviewing]);

  useEffect(() => {
    if (checkpoint && focusDraftNext.current) {
      focusDraftNext.current = false;
      draftHeading.current?.focus();
    }
  }, [checkpoint]);

  useEffect(() => {
    if (confirmingEnd) confirmHeading.current?.focus();
  }, [confirmingEnd]);

  useEffect(() => {
    if (confirmingDelete) deleteHeading.current?.focus();
  }, [confirmingDelete]);

  useEffect(() => {
    if (reviewing) previewHeading.current?.focus();
  }, [reviewing]);

  // After regenerating, the new checkpoint arrives a moment after the review opens.
  useEffect(() => {
    if (focusPreviewNext.current && reviewing && checkpoint && !checkpoint.draft) {
      focusPreviewNext.current = false;
      previewHeading.current?.focus();
    }
  }, [checkpoint, reviewing]);

  // Leaving the drafting states ends any review.
  useEffect(() => {
    if (!showsDraft) setReviewing(false);
  }, [showsDraft]);

  async function perform(name: string, work: () => Promise<unknown>, success: string): Promise<boolean> {
    setBusy(name);
    setActionError(null);
    setStatus("");
    let succeeded = false;
    try {
      await work();
      setStatus(success);
      succeeded = true;
    } catch (failure) {
      setActionError(toDomainError(failure).message);
    } finally {
      // The state may have changed even when the action failed.
      await onChanged();
      await loadEvents();
      setBusy(null);
    }
    return succeeded;
  }

  const saveMyPlace = () => {
    focusDraftNext.current = true;
    return perform("save", () => checkpointsApi.savePlace(thread.id), "Checkpoint saved on this computer.");
  };
  const endThread = () => {
    setConfirmingEnd(false);
    focusDraftNext.current = true;
    return perform("end", () => threadsApi.end(thread.id), "Thread ended and checkpoint saved.");
  };

  const deleteThread = async () => {
    setBusy("delete");
    setActionError(null);
    try {
      await threadsApi.delete(thread.id);
      setConfirmingDelete(false);
      await onChanged();
      onDeleted();
    } catch (failure) {
      setActionError(toDomainError(failure).message);
    } finally {
      setBusy(null);
    }
  };

  const discardDraft = async () => {
    if (!checkpoint) return;
    await perform("discard", () => checkpointsApi.discard(checkpoint.id), "Draft discarded. Recording has resumed.");
    setReviewing(false);
    document.getElementById("screen-title")?.focus();
  };

  /** Throws the unapproved draft away, saves the place again, and returns to the payload review. */
  const regenerate = async () => {
    if (!checkpoint) return;
    const done = await perform(
      "regenerate",
      async () => {
        await checkpointsApi.discard(checkpoint.id);
        await checkpointsApi.savePlace(thread.id);
      },
      "Saved your place again. Review what will be sent to generate a new capsule.",
    );
    if (done) {
      focusPreviewNext.current = true;
      setReviewing(true);
    }
  };

  const approved = async (result: ApprovalResult) => {
    focusCapsuleNext.current = true;
    setCorrections(result.corrections);
    setCheckpoint(result.checkpoint);
    setStatus(
      result.corrections.length === 0
        ? "Capsule approved as the model drafted it."
        : `Capsule approved with ${pluralize(result.corrections.length, "recorded correction")}.`,
    );
    await onChanged();
  };

  const state = STATE_DISPLAY[thread.state];
  const disabled = busy !== null;

  return (
    <>
      <h1 id="screen-title" tabIndex={-1}>
        {thread.title}
      </h1>
      <p className="state">
        <span aria-hidden="true">{state.glyph} </span>
        {state.label}
      </p>
      <p className="visually-hidden" role="status" aria-live="polite">
        {status}
      </p>
      {status ? (
        <p className="hint" aria-hidden="true">
          ✓ {status}
        </p>
      ) : null}
      {actionError ? (
        <p className="form-error" role="alert">
          <span aria-hidden="true">⚠ </span>
          {actionError}
        </p>
      ) : null}

      <section className="panel" aria-labelledby="goal-title">
        <h2 id="goal-title">Goal</h2>
        <p className="preformatted">{thread.goal}</p>
        <p className="hint">
          {project ? `${project.displayName} · ` : ""}Started {formatTime(thread.startedAt)}
        </p>
      </section>

      {confirmingEnd ? (
        <section className="panel confirm" role="group" aria-labelledby="confirm-title">
          <h2 id="confirm-title" tabIndex={-1} ref={confirmHeading}>
            End this thread?
          </h2>
          <p>ThreadBack will stop recording and save a checkpoint draft of what happened.</p>
          <div className="actions">
            <button type="button" className="primary" onClick={endThread} disabled={disabled}>
              End thread and save checkpoint
            </button>
            <button type="button" onClick={() => setConfirmingEnd(false)}>
              Keep working
            </button>
          </div>
        </section>
      ) : confirmingDelete ? (
        <section className="panel confirm" role="group" aria-labelledby="delete-confirm-title">
          <h2 id="delete-confirm-title" tabIndex={-1} ref={deleteHeading}>
            Permanently delete this thread?
          </h2>
          <p>
            This permanently deletes the thread, all recorded events, checkpoint drafts, approved capsules, corrections,
            and exported files on this computer. This cannot be undone.
          </p>
          <div className="actions">
            <button type="button" className="danger" onClick={deleteThread} disabled={disabled}>
              {busy === "delete" ? "Deleting…" : "Delete thread permanently"}
            </button>
            <button type="button" onClick={() => setConfirmingDelete(false)} disabled={disabled}>
              Cancel
            </button>
          </div>
        </section>
      ) : (
        <div className="actions" role="group" aria-label="Thread actions">
          {recording ? (
            <>
              <button type="button" className="primary" onClick={saveMyPlace} disabled={disabled} aria-busy={busy === "save"}>
                {busy === "save" ? "Saving…" : "Save My Place"}
              </button>
              <button type="button" disabled={disabled} onClick={() => perform("pause", () => threadsApi.pause(thread.id), "Paused. Nothing is being recorded.")}>
                Pause
              </button>
              <button type="button" disabled={disabled} onClick={() => perform("snapshot", () => eventsApi.captureGit(thread.id), "Git snapshot recorded.")}>
                Take Git snapshot
              </button>
              <button type="button" disabled={disabled} onClick={() => setConfirmingEnd(true)}>
                End thread…
              </button>
            </>
          ) : null}
          {thread.state === "PAUSED" ? (
            <>
              <button type="button" className="primary" disabled={disabled} onClick={() => perform("resume", () => threadsApi.resume(thread.id), "Recording resumed.")}>
                Resume
              </button>
              <button type="button" disabled={disabled} onClick={() => setConfirmingEnd(true)}>
                End thread…
              </button>
            </>
          ) : null}
          {thread.state === "READY" ? (
            <button
              type="button"
              className="primary"
              disabled={disabled}
              onClick={() => perform("resume", () => threadsApi.resume(thread.id), "Thread resumed. New evidence interval started.")}
            >
              Resume thread
            </button>
          ) : null}
          {canArchive ? (
            <button
              type="button"
              disabled={disabled}
              onClick={() => perform("archive", () => threadsApi.archive(thread.id), "Thread archived.")}
            >
              Archive thread
            </button>
          ) : null}
          <button type="button" disabled={disabled} onClick={() => setConfirmingDelete(true)}>
            Delete thread…
          </button>
        </div>
      )}

      {recording ? (
        <p className="hint">
          Record a command from the project folder with <code>tb run -- &lt;command&gt;</code>. It appears below.
        </p>
      ) : null}

      {showsDraft && checkpoint ? (
        reviewing && !checkpoint.draft ? (
          <PayloadPreviewPanel
            ref={previewHeading}
            checkpoint={checkpoint}
            busy={disabled}
            onCancel={() => {
              focusDraftNext.current = true;
              setReviewing(false);
              draftHeading.current?.focus();
            }}
            onDiscard={discardDraft}
            onOpenSettings={onOpenSettings}
            onGenerated={async (generated) => {
              // Focus moves once the capsule has been rendered, not before.
              focusCapsuleNext.current = true;
              setCheckpoint(generated);
              setReviewing(false);
              setStatus("Capsule draft generated. Nothing else was sent.");
              await onChanged();
            }}
          />
        ) : (
          <>
            {checkpoint.status === "approved" && checkpoint.approved ? (
              <ApprovedCapsule ref={capsuleHeading} checkpoint={checkpoint} corrections={corrections} />
            ) : checkpoint.draft ? (
              <CapsuleReview
                key={checkpoint.id}
                ref={capsuleHeading}
                checkpoint={checkpoint}
                busy={disabled}
                onApproved={approved}
                onRegenerate={regenerate}
              />
            ) : null}
            <CheckpointDraftView
              ref={draftHeading}
              checkpoint={checkpoint}
              generated={Boolean(checkpoint.draft)}
              canDiscard={checkpoint.status !== "approved"}
              busy={disabled}
              onReview={() => setReviewing(true)}
              onDiscard={discardDraft}
            />
          </>
        )
      ) : null}

      <NoteForm
        recording={recording}
        onAdd={async (note) => {
          await eventsApi.addNote(thread.id, note);
          setStatus("Note added.");
          await loadEvents();
        }}
      />

      <section aria-labelledby="timeline-title">
        <div className="section-head">
          <h2 id="timeline-title">Evidence</h2>
          <button type="button" onClick={() => void loadEvents()}>
            Refresh
          </button>
        </div>
        {loadError ? (
          <p className="form-error" role="alert">
            <span aria-hidden="true">⚠ </span>
            {loadError}
          </p>
        ) : events === null ? (
          <p className="empty" role="status">
            Loading evidence…
          </p>
        ) : (
          <>
            <p className="hint">{pluralize(events.length, "event")} recorded.</p>
            <Timeline items={eventItems(events)} />
          </>
        )}
      </section>
    </>
  );
}
