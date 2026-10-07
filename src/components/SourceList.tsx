import { useState } from "react";
import type { CheckpointEntry } from "../bindings/contracts";
import { entryItems, summarizeEntry } from "../lib/format";
import { Timeline } from "./Timeline";

interface SourceListProps {
  ids: string[];
  /** The checkpoint's evidence, which every cited ID must resolve to. */
  entries: CheckpointEntry[];
}

/**
 * The evidence a claim cites. Each source is a button that opens the recorded
 * entry underneath, so a claim can be checked without leaving the capsule.
 */
export function SourceList({ ids, entries }: SourceListProps) {
  const [open, setOpen] = useState<string | null>(null);
  if (ids.length === 0) return <p className="hint">No cited events.</p>;

  const byId = new Map(entries.map((entry) => [entry.eventId, entry]));
  const opened = open ? (byId.get(open) ?? null) : null;

  return (
    <div className="sources">
      <ul className="chips" aria-label="Sources">
        {ids.map((id, index) => {
          const entry = byId.get(id);
          const name = entry ? `Source ${index + 1}: ${summarizeEntry(entry).kind}` : `Source ${index + 1}: not found`;
          return (
            <li key={id}>
              <button
                type="button"
                className="chip"
                aria-expanded={open === id}
                onClick={() => setOpen(open === id ? null : id)}
              >
                <span aria-hidden="true">{entry ? "▸ " : "✕ "}</span>
                {name}
              </button>
            </li>
          );
        })}
      </ul>
      {open ? (
        opened ? (
          <Timeline items={entryItems([opened])} label={`Evidence for the source you opened`} />
        ) : (
          <p className="form-error" role="alert">
            <span aria-hidden="true">⚠ </span>
            This source is not an event in this checkpoint, so it cannot back a verified fact.
          </p>
        )
      ) : null}
    </div>
  );
}
