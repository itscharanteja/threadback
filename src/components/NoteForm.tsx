import { useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import type { AddNoteInput, NoteLabel } from "../bindings/contracts";
import { toDomainError } from "../api/errors";
import { NOTE_LABELS } from "../lib/format";
import { Field } from "./Field";

const LABEL_HELP: Record<NoteLabel, string> = {
  fact: "Something you observed and can point to.",
  hypothesis: "An explanation you suspect but have not proven.",
  question: "Something you still need to find out.",
  next_step: "What you plan to try next.",
};

interface NoteFormProps {
  /** Notes are only recorded while the thread is recording. */
  recording: boolean;
  onAdd: (note: AddNoteInput) => Promise<void>;
}

/** Adds a labelled note. The label is the user's call, so none is pre-selected. */
export function NoteForm({ recording, onAdd }: NoteFormProps) {
  const [label, setLabel] = useState<NoteLabel | null>(null);
  const [text, setText] = useState("");
  const [errors, setErrors] = useState<{ label?: string; text?: string; form?: string }>({});
  const [busy, setBusy] = useState(false);
  const textRef = useRef<HTMLTextAreaElement>(null);

  async function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!recording || busy) return;
    const problems: typeof errors = {};
    if (!label) problems.label = "Choose what kind of note this is.";
    if (!text.trim()) problems.text = "Write the note first.";
    if (problems.label || problems.text) {
      setErrors(problems);
      document.getElementById(problems.label ? "note-label-fact" : "note-text")?.focus();
      return;
    }
    setBusy(true);
    setErrors({});
    try {
      await onAdd({ text, label: label! });
      setText("");
      textRef.current?.focus();
    } catch (failure) {
      const domain = toDomainError(failure);
      setErrors(domain.field === "text" ? { text: domain.message } : { form: domain.message });
    } finally {
      setBusy(false);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      void submit();
    }
  }

  return (
    <form className="panel" onSubmit={submit} noValidate aria-labelledby="note-form-title">
      <h2 id="note-form-title">Add a note</h2>
      {!recording ? (
        <p className="hint" role="status">
          Notes are recorded only while the thread is active. Resume it to add one.
        </p>
      ) : null}
      {errors.form ? (
        <p className="form-error" role="alert">
          <span aria-hidden="true">⚠ </span>
          {errors.form}
        </p>
      ) : null}

      <fieldset className="labels" disabled={!recording} aria-describedby={errors.label ? "note-label-error" : undefined}>
        <legend>
          Kind of note <span className="required">(required)</span>
        </legend>
        {(Object.keys(NOTE_LABELS) as NoteLabel[]).map((option) => (
          <label key={option} className="radio">
            <input
              id={`note-label-${option}`}
              type="radio"
              name="note-label"
              value={option}
              checked={label === option}
              onChange={() => setLabel(option)}
            />
            <span>
              <strong>{NOTE_LABELS[option]}</strong>
              <span className="hint"> — {LABEL_HELP[option]}</span>
            </span>
          </label>
        ))}
        {errors.label ? (
          <p className="field-error" id="note-label-error">
            <span aria-hidden="true">⚠ </span>
            {errors.label}
          </p>
        ) : null}
      </fieldset>

      <Field id="note-text" label="Note" required error={errors.text} hint="Press Ctrl or ⌘ + Enter to add.">
        {(aria) => (
          <textarea
            {...aria}
            ref={textRef}
            rows={3}
            value={text}
            disabled={!recording}
            onChange={(event) => setText(event.target.value)}
            onKeyDown={onKeyDown}
          />
        )}
      </Field>

      <div className="actions">
        <button type="submit" className="primary" disabled={!recording || busy} aria-busy={busy}>
          {busy ? "Adding…" : "Add note"}
        </button>
      </div>
    </form>
  );
}
