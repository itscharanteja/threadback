import { useEffect, useState, type FormEvent } from "react";
import type { ConfigurationStatus, ConnectionTestResult, ModelConfigInput, StructuredOutput } from "../bindings/contracts";
import { configuration } from "../api/commands";
import { Field } from "../components/Field";
import { useFieldErrors } from "../lib/useFieldErrors";

const FIELDS = ["endpoint", "modelId", "timeoutSecs", "apiKey"];

const FORMATS: { value: StructuredOutput; label: string }[] = [
  { value: "json_schema", label: "JSON schema (strict) — recommended" },
  { value: "json_object", label: "JSON object" },
  { value: "off", label: "Off — rely on the prompt alone" },
];

/**
 * The model connection. The API key is write-only: it is never loaded into
 * this form, never shown, and goes straight to the Keychain when saved.
 */
export function ModelSettingsScreen() {
  const [status, setStatus] = useState<ConfigurationStatus | null>(null);
  const [endpoint, setEndpoint] = useState("");
  const [modelId, setModelId] = useState("");
  const [timeoutSecs, setTimeoutSecs] = useState("60");
  const [format, setFormat] = useState<StructuredOutput>("json_schema");
  const [apiKey, setApiKey] = useState("");
  const [saved, setSaved] = useState("");
  const [test, setTest] = useState<ConnectionTestResult | null>(null);
  const [busy, setBusy] = useState<"save" | "test" | "remove" | null>(null);
  const { errors, formError, fail, failFields, clear } = useFieldErrors(FIELDS);

  function show(next: ConfigurationStatus) {
    setStatus(next);
    setEndpoint(next.endpoint);
    setModelId(next.modelId);
    setTimeoutSecs(String(next.timeoutSecs));
    setFormat(next.structuredOutput);
  }

  useEffect(() => {
    configuration.status().then(show).catch(fail);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  function input(): ModelConfigInput | null {
    const seconds = Number(timeoutSecs);
    if (!Number.isInteger(seconds)) {
      failFields({ timeoutSecs: "Enter the timeout as a whole number of seconds." });
      return null;
    }
    return {
      endpoint,
      modelId,
      timeoutSecs: seconds,
      structuredOutput: format,
      apiKey: apiKey.trim() ? apiKey : null,
    };
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    const body = input();
    if (!body) return;
    setBusy("save");
    clear();
    setSaved("");
    setTest(null);
    try {
      show(await configuration.store(body));
      setApiKey("");
      setSaved(body.apiKey ? "Saved. The API key is now in the macOS Keychain." : "Saved.");
    } catch (failure) {
      fail(failure);
    } finally {
      setBusy(null);
    }
  }

  async function testConnection() {
    const body = input();
    if (!body) return;
    setBusy("test");
    clear();
    setSaved("");
    setTest(null);
    try {
      setTest(await configuration.test(body));
    } catch (failure) {
      fail(failure);
    } finally {
      setBusy(null);
    }
  }

  async function removeKey() {
    setBusy("remove");
    clear();
    setSaved("");
    setTest(null);
    try {
      show(await configuration.removeApiKey());
      setSaved("The API key was removed from the Keychain.");
    } catch (failure) {
      setBusy(null);
      fail(failure);
      return;
    }
    setBusy(null);
  }

  return (
    <form onSubmit={save} noValidate aria-labelledby="screen-title">
      <h1 id="screen-title" tabIndex={-1}>
        Model connection
      </h1>
      <p className="lede">
        ThreadBack turns a checkpoint into a resume capsule with an NVIDIA Nemotron model on Nebius Token
        Factory. Evidence is sent only after you review it and press Generate.
      </p>

      {formError ? (
        <p className="form-error" id="form-error" role="alert" tabIndex={-1}>
          <span aria-hidden="true">⚠ </span>
          {formError}
        </p>
      ) : null}

      <p className="state" role="status">
        <span aria-hidden="true">{status?.hasApiKey ? "✓ " : "○ "}</span>
        {status === null
          ? "Loading…"
          : status.hasApiKey
            ? "An API key is stored in the macOS Keychain."
            : "No API key is stored yet."}
      </p>

      <Field
        id="apiKey"
        label={status?.hasApiKey ? "Replace the API key" : "API key"}
        required={!status?.hasApiKey}
        error={errors.apiKey}
        hint="Kept only in the macOS Keychain. It is never shown again, never saved in ThreadBack's database, and never exported."
      >
        {(aria) => (
          <input
            {...aria}
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={apiKey}
            placeholder={status?.hasApiKey ? "Leave empty to keep the stored key" : ""}
            onChange={(event) => setApiKey(event.target.value)}
          />
        )}
      </Field>

      <Field
        id="endpoint"
        label="Endpoint"
        required
        error={errors.endpoint}
        hint="Must use https. Requests go to this address followed by /chat/completions."
      >
        {(aria) => <input {...aria} type="text" value={endpoint} onChange={(event) => setEndpoint(event.target.value)} />}
      </Field>

      <Field id="modelId" label="Model ID" required error={errors.modelId} hint="For example nvidia/nemotron-3-super-120b-a12b.">
        {(aria) => <input {...aria} type="text" value={modelId} onChange={(event) => setModelId(event.target.value)} />}
      </Field>

      <Field id="timeoutSecs" label="Timeout (seconds)" required error={errors.timeoutSecs} hint="Between 5 and 300.">
        {(aria) => (
          <input {...aria} type="text" inputMode="numeric" value={timeoutSecs} onChange={(event) => setTimeoutSecs(event.target.value)} />
        )}
      </Field>

      <Field
        id="structuredOutput"
        label="Ask for JSON using"
        hint="If the service rejects the chosen format, ThreadBack steps down to a simpler one on its own."
      >
        {(aria) => (
          <select {...aria} value={format} onChange={(event) => setFormat(event.target.value as StructuredOutput)}>
            {FORMATS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        )}
      </Field>

      {status ? (
        <p className="hint">
          Evidence would be sent to <strong>{status.destinationHost}</strong> using <strong>{status.modelId}</strong>.
        </p>
      ) : null}

      {saved ? (
        <p className="hint" role="status">
          <span aria-hidden="true">✓ </span>
          {saved}
        </p>
      ) : null}
      {test ? (
        <p className={test.ok ? "hint" : "form-error"} role={test.ok ? "status" : "alert"}>
          <span aria-hidden="true">{test.ok ? "✓ " : "⚠ "}</span>
          {test.message}
        </p>
      ) : null}

      <div className="actions">
        <button type="submit" className="primary" disabled={busy !== null} aria-busy={busy === "save"}>
          {busy === "save" ? "Saving…" : "Save"}
        </button>
        <button type="button" disabled={busy !== null} aria-busy={busy === "test"} onClick={testConnection}>
          {busy === "test" ? "Testing…" : "Test connection"}
        </button>
        {status?.hasApiKey ? (
          <button type="button" disabled={busy !== null} onClick={removeKey}>
            Remove stored key
          </button>
        ) : null}
      </div>
      <p className="hint">
        Testing sends one tiny question that contains none of your evidence, using the values in this form.
      </p>
    </form>
  );
}

