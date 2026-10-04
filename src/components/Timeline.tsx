import type { TimelineItem } from "../lib/format";
import { formatTime } from "../lib/format";

interface TimelineProps {
  items: TimelineItem[];
  label?: string;
  emptyText?: string;
}

export function Timeline({ items, label = "Evidence timeline, oldest first", emptyText = "No evidence recorded yet." }: TimelineProps) {
  if (items.length === 0) {
    return <p className="empty">{emptyText}</p>;
  }
  return (
    <ol className="timeline" aria-label={label}>
      {items.map(({ id, timestamp, summary, redacted }) => (
        <li key={id} className="timeline-item">
          <div className="timeline-meta">
            <span className="tag">{summary.kind}</span>
            {redacted ? (
              <span className="tag tag-redacted">
                <span aria-hidden="true">🔒 </span>Credential redacted
              </span>
            ) : null}
            <time dateTime={timestamp}>{formatTime(timestamp)}</time>
          </div>
          <p className={summary.preformatted ? "preformatted" : undefined}>{summary.text}</p>
          {summary.detail ? <p className="detail">{summary.detail}</p> : null}
          {summary.outputs?.map((output) => (
            <details key={output.label} className="output">
              <summary>
                {output.label}
                {output.note ? <span className="hint"> — {output.note}</span> : null}
              </summary>
              {output.text ? <pre>{output.text}</pre> : null}
            </details>
          ))}
        </li>
      ))}
    </ol>
  );
}
