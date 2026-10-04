import { useEffect, useRef, useState } from "react";

interface DiscardDraftControlProps {
  onConfirm: () => Promise<void> | void;
  busy: boolean;
}

/** Throws a draft away only after an explicit second step. */
export function DiscardDraftControl({ onConfirm, busy }: DiscardDraftControlProps) {
  const [confirming, setConfirming] = useState(false);
  const confirmButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (confirming) confirmButton.current?.focus();
  }, [confirming]);

  if (!confirming) {
    return (
      <button type="button" disabled={busy} onClick={() => setConfirming(true)}>
        Discard draft and keep working…
      </button>
    );
  }
  return (
    <div className="confirm panel" role="group" aria-label="Confirm discarding the draft">
      <p>
        Discard this draft and keep working? Recording resumes and nothing is lost: the evidence stays in the
        thread, and nothing was sent.
      </p>
      <div className="actions">
        <button
          type="button"
          className="primary"
          ref={confirmButton}
          disabled={busy}
          onClick={() => {
            setConfirming(false);
            void onConfirm();
          }}
        >
          Discard draft
        </button>
        <button type="button" onClick={() => setConfirming(false)}>
          Keep draft
        </button>
      </div>
    </div>
  );
}
