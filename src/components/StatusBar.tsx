import type { WorkThread } from "../bindings/contracts";

interface StatusBarProps {
  coreReady: boolean;
  online: boolean;
  recordingThread: WorkThread | undefined;
  onOpenThread: (thread: WorkThread) => void;
}

/** Capture, network, and core state, each stated in words with a glyph. */
export function StatusBar({ coreReady, online, recordingThread, onOpenThread }: StatusBarProps) {
  return (
    <section className="status-bar" aria-label="Status">
      <p className={recordingThread ? "pill pill-on" : "pill"}>
        <span aria-hidden="true">{recordingThread ? "● " : "○ "}</span>
        {recordingThread ? (
          <>
            Capturing evidence:{" "}
            <button type="button" className="link" onClick={() => onOpenThread(recordingThread)}>
              {recordingThread.title}
            </button>
          </>
        ) : (
          "Not capturing — nothing is recorded without an active thread"
        )}
      </p>
      <p className="pill">
        <span aria-hidden="true">{online ? "✓ " : "✕ "}</span>
        {online ? "Network available" : "Offline — checkpoints still work locally"}
      </p>
      <p className="pill">
        <span aria-hidden="true">{coreReady ? "✓ " : "… "}</span>
        {coreReady ? "Local data ready" : "Starting local data"}
      </p>
    </section>
  );
}
