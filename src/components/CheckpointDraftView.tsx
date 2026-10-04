import { forwardRef } from "react";
import type { Checkpoint, CheckpointPayload } from "../bindings/contracts";
import { TRIGGER_LABELS, entryItems, formatTime, pluralize } from "../lib/format";
import { Timeline } from "./Timeline";

/** A saved checkpoint, readable with no model or network. */
export const CheckpointDraftView = forwardRef<HTMLHeadingElement, { checkpoint: Checkpoint }>(
  function CheckpointDraftView({ checkpoint }, headingRef) {
    const payload = checkpoint.payload as unknown as CheckpointPayload;
    const { omissions, finalSnapshot } = payload;
    const leftOut: string[] = [];
    if (omissions.duplicatesRemoved > 0) leftOut.push(`${pluralize(omissions.duplicatesRemoved, "repeated entry", "repeated entries")} merged`);
    if (omissions.overBudgetRemoved > 0) leftOut.push(`${pluralize(omissions.overBudgetRemoved, "entry", "entries")} dropped to fit the size limit`);
    if (omissions.notesShortened > 0) leftOut.push(`${pluralize(omissions.notesShortened, "long note")} shortened`);
    if (omissions.unreadable > 0) leftOut.push(`${pluralize(omissions.unreadable, "entry", "entries")} could not be read`);

    return (
      <section className="panel" aria-labelledby="draft-title">
        <h2 id="draft-title" tabIndex={-1} ref={headingRef}>
          Checkpoint draft
        </h2>
        <p className="hint">
          Saved {formatTime(checkpoint.createdAt)} · {TRIGGER_LABELS[payload.trigger]} · kept on this computer only.
          Nothing has been sent anywhere.
        </p>

        <dl className="facts">
          <div>
            <dt>Evidence included</dt>
            <dd>
              {payload.entries.length} of {pluralize(payload.intervalEventCount, "event")} in this interval
            </dd>
          </div>
          <div>
            <dt>Final Git snapshot</dt>
            <dd>
              {finalSnapshot.captured ? (
                <>
                  <span aria-hidden="true">✓ </span>Taken when you saved
                </>
              ) : (
                <>
                  <span aria-hidden="true">✕ </span>Not taken — {finalSnapshot.reason ?? "no reason recorded"}
                </>
              )}
            </dd>
          </div>
          <div>
            <dt>Left out</dt>
            <dd>{leftOut.length > 0 ? leftOut.join("; ") : "Nothing"}</dd>
          </div>
        </dl>

        <Timeline items={entryItems(payload.entries)} label="Checkpoint evidence, oldest first" />

        <p className="hint">
          Turning this draft into a resume capsule needs the model connection, which is not set up yet. This
          draft is your saved place until then.
        </p>
      </section>
    );
  },
);
