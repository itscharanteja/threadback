import type { ReactNode } from "react";

interface FieldProps {
  id: string;
  label: string;
  hint?: string;
  error?: string;
  required?: boolean;
  children: (aria: {
    id: string;
    "aria-describedby"?: string;
    "aria-invalid"?: true;
    "aria-required"?: true;
  }) => ReactNode;
}

/** A labelled control whose hint and error are announced with it. */
export function Field({ id, label, hint, error, required, children }: FieldProps) {
  const describedBy = [hint ? `${id}-hint` : null, error ? `${id}-error` : null]
    .filter(Boolean)
    .join(" ");
  return (
    <div className="field">
      <label htmlFor={id}>
        {label}
        {required ? <span className="required"> (required)</span> : <span className="optional"> (optional)</span>}
      </label>
      {hint ? (
        <p className="hint" id={`${id}-hint`}>
          {hint}
        </p>
      ) : null}
      {children({
        id,
        "aria-describedby": describedBy || undefined,
        "aria-invalid": error ? true : undefined,
        "aria-required": required ? true : undefined,
      })}
      {error ? (
        <p className="field-error" id={`${id}-error`}>
          <span aria-hidden="true">⚠ </span>
          {error}
        </p>
      ) : null}
    </div>
  );
}
