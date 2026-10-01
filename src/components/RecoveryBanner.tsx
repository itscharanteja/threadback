import type { RecoveryReport } from "../bindings/contracts";
import { formatTime } from "../lib/format";

interface RecoveryBannerProps {
  reports: RecoveryReport[];
  onDismiss: (threadId: string) => void;
}

export function RecoveryBanner({ reports, onDismiss }: RecoveryBannerProps) {
  if (reports.length === 0) return null;
  return (
    <section className="banner" role="status" aria-label="Interrupted threads">
      {reports.map((report) => (
        <div key={report.threadId.valueOf()} className="banner-row">
          <p>
            <span aria-hidden="true">⚠ </span>
            <strong>{report.threadTitle}</strong> was still recording when ThreadBack last stopped, so it
            was paused. Evidence is complete up to{" "}
            {report.lastEventAt ? formatTime(report.lastEventAt) : "its start"}; nothing was captured
            between then and {formatTime(report.recoveredAt)}.
          </p>
          <button type="button" onClick={() => onDismiss(report.threadId)}>
            Dismiss
          </button>
        </div>
      ))}
    </section>
  );
}
