import type { ThreadEvent } from "../bindings/contracts";
import { formatTime, summarizeEvent } from "../lib/format";

export function Timeline({ events }: { events: ThreadEvent[] }) {
  if (events.length === 0) {
    return <p className="empty">No evidence recorded yet.</p>;
  }
  return (
    <ol className="timeline" aria-label="Evidence timeline, oldest first">
      {events.map((event) => {
        const summary = summarizeEvent(event);
        return (
          <li key={event.id} className="timeline-item">
            <div className="timeline-meta">
              <span className="tag">{summary.kind}</span>
              <time dateTime={event.timestamp}>{formatTime(event.timestamp)}</time>
            </div>
            <p className={summary.preformatted ? "preformatted" : undefined}>{summary.text}</p>
            {summary.detail ? <p className="detail">{summary.detail}</p> : null}
          </li>
        );
      })}
    </ol>
  );
}
