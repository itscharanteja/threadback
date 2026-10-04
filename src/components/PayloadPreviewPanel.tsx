import { forwardRef, useCallback, useEffect, useRef, useState } from "react";
import type { Checkpoint, PayloadPreview, PayloadPreviewEntry } from "../bindings/contracts";
import { checkpoints as checkpointsApi } from "../api/commands";
import { toDomainError } from "../api/errors";
import { formatBytes, formatTime, pluralize, summarizeEntry } from "../lib/format";
import { DiscardDraftControl } from "./DiscardDraftControl";

const DISPOSITION: Record<PayloadPreviewEntry["disposition"], { glyph: string; label: string }> = {
  included: { glyph: "✓", label: "Will be sent" },
  excluded: { glyph: "✕", label: "Excluded by you" },
  blocked: { glyph: "⚠", label: "Blocked — contains a credential" },
  local_only: { glyph: "◇", label: "Stays on this computer" },
};

function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

interface PayloadPreviewPanelProps {
  checkpoint: Checkpoint;
  onCancel: () => void;
  onDiscard: () => Promise<void>;
  busy: boolean;
}

/**
 * The exact payload for a checkpoint, entry by entry. Looking, excluding,
 * and cancelling send nothing; only the Generate button asks for a model
 * request, and the server refuses it unless the payload is clean and is the
 * one shown here.
 */
export const PayloadPreviewPanel = forwardRef<HTMLHeadingElement, PayloadPreviewPanelProps>(
  function PayloadPreviewPanel({ checkpoint, onCancel, onDiscard, busy }, headingRef) {
    const [excluded, setExcluded] = useState<string[]>([]);
    const [preview, setPreview] = useState<PayloadPreview | null>(null);
    const [loadedFor, setLoadedFor] = useState<string | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [generating, setGenerating] = useState(false);
    const [outcome, setOutcome] = useState<string | null>(null);
    const latest = useRef(0);
    const key = excluded.join(",");

    const load = useCallback(
      async (ids: string[]) => {
        const mine = ++latest.current;
        try {
          const next = await checkpointsApi.preview(checkpoint.id, ids);
          if (mine !== latest.current) return;
          setPreview(next);
          setLoadedFor(ids.join(","));
          setError(null);
        } catch (failure) {
          if (mine === latest.current) setError(toDomainError(failure).message);
        }
      },
      [checkpoint.id],
    );

    useEffect(() => {
      void load(excluded);
    }, [load, excluded]);

    const toggle = (id: string) => {
      setOutcome(null);
      setExcluded((current) => (current.includes(id) ? current.filter((other) => other !== id) : [...current, id]));
    };

    // The preview must match the current selection before it can be sent.
    const fresh = preview !== null && loadedFor === key;
    const canGenerate = fresh && preview.canGenerate && !generating && !busy;

    async function generate() {
      if (!canGenerate || !preview) return;
      setGenerating(true);
      setOutcome(null);
      try {
        await checkpointsApi.generate(checkpoint.id, excluded, preview.payloadHash);
        setOutcome("Generation was requested.");
      } catch (failure) {
        const domain = toDomainError(failure);
        setOutcome(domain.message);
        if (domain.code === "PAYLOAD_CHANGED" || domain.code === "PAYLOAD_BLOCKED") await load(excluded);
      } finally {
        setGenerating(false);
      }
    }

    let hint = "Pressing Generate sends exactly the payload shown here to the model, and nothing else.";
    if (preview && !fresh) hint = "Updating the payload…";
    else if (preview?.hasBlockingFindings) {
      hint = `Generate is unavailable: ${pluralize(preview.counts.blocked, "entry", "entries")} contain${preview.counts.blocked === 1 ? "s" : ""} a credential. Exclude ${preview.counts.blocked === 1 ? "it" : "them"} to continue.`;
    } else if (preview && preview.counts.included === 0) {
      hint = "Generate is unavailable: nothing is selected. Include at least one entry.";
    }

    return (
      <section className="panel" aria-labelledby="preview-title">
        <h2 id="preview-title" tabIndex={-1} ref={headingRef}>
          Review what will be sent
        </h2>
        <p>
          <strong>Nothing has been sent.</strong> ThreadBack contacts the model only when you press Generate,
          and it sends only what is listed here. Looking, excluding entries, and cancelling send nothing.
        </p>

        {error ? (
          <p className="form-error" role="alert">
            <span aria-hidden="true">⚠ </span>
            {error}
          </p>
        ) : null}

        {preview ? (
          <>
            <p className="summary-line" role="status" aria-live="polite">
              {pluralize(preview.counts.included, "entry", "entries")} will be sent
              {" · "}
              {formatBytes(preview.totalSizeBytes)} in total
              {preview.counts.excluded > 0 ? ` · ${preview.counts.excluded} excluded` : ""}
              {preview.counts.localOnly > 0 ? ` · ${preview.counts.localOnly} stay on this computer` : ""}
              {preview.counts.blocked > 0 ? ` · ${preview.counts.blocked} blocked` : ""}
            </p>

            {preview.hasBlockingFindings ? (
              <p className="form-error" role="alert">
                <span aria-hidden="true">⚠ </span>
                {pluralize(preview.counts.blocked, "entry", "entries")} contain{preview.counts.blocked === 1 ? "s" : ""} a
                credential and cannot be sent. Exclude {preview.counts.blocked === 1 ? "it" : "them"} to continue.
              </p>
            ) : null}

            {preview.notices.length > 0 ? (
              <ul className="plain notices" aria-label="Changes made before sending">
                {preview.notices.map((notice) => (
                  <li key={notice}>{notice}</li>
                ))}
              </ul>
            ) : null}

            <dl className="facts">
              <div>
                <dt>Thread title</dt>
                <dd className="preformatted">{preview.threadTitle}</dd>
              </div>
              <div>
                <dt>Goal</dt>
                <dd className="preformatted">{preview.goal}</dd>
              </div>
            </dl>

            <ul className="preview-entries" aria-label="Entries in the payload">
              {preview.entries.map((entry) => {
                const local = entry.disposition === "local_only";
                const isExcluded = excluded.includes(entry.eventId);
                const summary = summarizeEntry({
                  eventId: entry.eventId,
                  timestamp: entry.timestamp,
                  kind: entry.kind,
                  content: entry.value,
                });
                const shown = DISPOSITION[entry.disposition];
                const redacted = JSON.stringify(entry.value).includes("[REDACTED:");
                const checkboxId = `send-${entry.eventId}`;
                return (
                  <li key={entry.eventId} className={`preview-entry preview-${entry.disposition}`}>
                    <div className="entry-head">
                      <input
                        id={checkboxId}
                        type="checkbox"
                        checked={!local && !isExcluded}
                        disabled={local}
                        aria-describedby={`${checkboxId}-state`}
                        onChange={() => toggle(entry.eventId)}
                      />
                      <label htmlFor={checkboxId}>
                        <strong>{summary.kind}</strong>
                        <span className="hint"> · {formatTime(entry.timestamp)}</span>
                        <span className="visually-hidden"> — send this entry</span>
                      </label>
                      <span className="tag">{formatBytes(entry.sizeBytes)}</span>
                      {redacted ? (
                        <span className="tag tag-redacted">
                          <span aria-hidden="true">🔒 </span>Credential redacted
                        </span>
                      ) : null}
                    </div>
                    <p className="entry-state" id={`${checkboxId}-state`}>
                      <span aria-hidden="true">{shown.glyph} </span>
                      {shown.label}
                      {entry.reason && entry.disposition !== "excluded" ? ` — ${entry.reason}` : ""}
                    </p>
                    <p className={summary.preformatted ? "preformatted" : undefined}>{summary.text}</p>
                    {summary.detail ? <p className="detail">{summary.detail}</p> : null}
                    <details className="output">
                      <summary>Exact content {entry.disposition === "included" ? "sent" : "(not sent)"}</summary>
                      <pre>{JSON.stringify(entry.value, null, 2)}</pre>
                    </details>
                  </li>
                );
              })}
            </ul>

            <details className="output">
              <summary>
                Show the complete payload as one document ({formatBytes(preview.totalSizeBytes)}, fingerprint{" "}
                {preview.payloadHash.slice(0, 12)})
              </summary>
              <pre>{prettyJson(preview.payloadJson)}</pre>
            </details>
          </>
        ) : error ? null : (
          <p className="empty" role="status">
            Preparing the payload…
          </p>
        )}

        {outcome ? (
          <p className="form-error" role="alert">
            <span aria-hidden="true">⚠ </span>
            {outcome}
          </p>
        ) : null}

        <p className="hint" id="generate-hint">
          {hint}
        </p>
        <div className="actions" role="group" aria-label="Payload actions">
          <button
            type="button"
            className="primary"
            aria-disabled={!canGenerate}
            aria-describedby="generate-hint"
            aria-busy={generating}
            onClick={generate}
          >
            {generating ? "Requesting…" : "Generate capsule"}
          </button>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <DiscardDraftControl onConfirm={onDiscard} busy={busy} />
        </div>
      </section>
    );
  },
);
