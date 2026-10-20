import type { TimelineItem } from "../lib/format";
import { formatClockTime, formatDate, formatTime, pluralize } from "../lib/format";

interface TimelineProps {
  items: TimelineItem[];
  label?: string;
  emptyText?: string;
}

export interface DateGroupedItems {
  dateKey: string;
  dateLabel: string;
  dateIso: string;
  items: TimelineItem[];
}

export function groupItemsByDate(items: TimelineItem[]): DateGroupedItems[] {
  // Order items latest first (newest timestamp first, oldest last)
  const orderedItems = [...items].reverse().sort((a, b) => {
    const timeA = new Date(a.timestamp).getTime();
    const timeB = new Date(b.timestamp).getTime();
    return timeB - timeA;
  });

  const groups: DateGroupedItems[] = [];
  let currentGroup: DateGroupedItems | null = null;

  for (const item of orderedItems) {
    const label = formatDate(item.timestamp);
    if (!currentGroup || currentGroup.dateLabel !== label) {
      currentGroup = {
        dateKey: `${label}-${item.id}`,
        dateLabel: label,
        dateIso: item.timestamp,
        items: [item],
      };
      groups.push(currentGroup);
    } else {
      currentGroup.items.push(item);
    }
  }

  return groups;
}

export function Timeline({
  items,
  label = "Evidence timeline, latest first",
  emptyText = "No evidence recorded yet.",
}: TimelineProps) {
  if (items.length === 0) {
    return <p className="empty">{emptyText}</p>;
  }

  const groups = groupItemsByDate(items);

  return (
    <ol className="timeline" aria-label={label}>
      {groups.map((group) => (
        <li key={group.dateKey} className="timeline-date-group">
          <div className="timeline-date-side">
            <div className="timeline-date-badge">
              <time className="timeline-date-label" dateTime={group.dateIso}>
                {group.dateLabel}
              </time>
              <span className="timeline-date-sub">{pluralize(group.items.length, "event")}</span>
            </div>
          </div>
          <div className="timeline-items-track">
            <ol className="timeline-items">
              {group.items.map(({ id, timestamp, summary, redacted }) => (
                <li key={id} className="timeline-item">
                  <div className="timeline-meta">
                    <time
                      dateTime={timestamp}
                      className="timeline-time-highlight"
                      title={formatTime(timestamp)}
                    >
                      {formatClockTime(timestamp)}
                    </time>
                    <span className="tag">{summary.kind}</span>
                    {redacted ? (
                      <span className="tag tag-redacted">
                        <span aria-hidden="true">🔒 </span>Credential redacted
                      </span>
                    ) : null}
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
          </div>
        </li>
      ))}
    </ol>
  );
}
