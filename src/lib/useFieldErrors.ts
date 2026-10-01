import { useCallback, useEffect, useState } from "react";
import { toDomainError } from "../api/errors";

/**
 * Collects per-field errors plus one form-level error, and moves focus to the
 * first field with a problem so keyboard and screen-reader users land on it.
 */
export function useFieldErrors(fieldOrder: string[]) {
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);

  const fail = useCallback(
    (error: unknown) => {
      const domain = toDomainError(error);
      if (domain.field && fieldOrder.includes(domain.field)) {
        setErrors({ [domain.field]: domain.message });
        setFormError(null);
      } else {
        setErrors({});
        setFormError(domain.message);
      }
      setAttempt((value) => value + 1);
    },
    [fieldOrder],
  );

  const failFields = useCallback((next: Record<string, string>) => {
    setErrors(next);
    setFormError(null);
    setAttempt((value) => value + 1);
  }, []);

  const clear = useCallback(() => {
    setErrors({});
    setFormError(null);
  }, []);

  useEffect(() => {
    if (attempt === 0) return;
    const first = fieldOrder.find((field) => errors[field]);
    if (first) document.getElementById(first)?.focus();
    else document.getElementById("form-error")?.focus();
  }, [attempt]); // eslint-disable-line react-hooks/exhaustive-deps

  return { errors, formError, fail, failFields, clear };
}
