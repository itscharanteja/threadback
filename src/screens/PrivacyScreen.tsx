import { useEffect, useRef, useState } from "react";
import type { Project, ProjectPolicy, RedactionTestResult, WorkThread } from "../bindings/contracts";
import { privacy as privacyApi, projects as projectsApi, threads as threadsApi } from "../api/commands";
import { toDomainError } from "../api/errors";
import { Field } from "../components/Field";
import { STATE_DISPLAY, pluralize } from "../lib/format";

interface PrivacyScreenProps {
  projects: Project[];
  threads?: WorkThread[];
  recordingThread?: WorkThread | null;
  onChanged?: () => Promise<void> | void;
}

const linesToArray = (value: string): string[] =>
  value
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

/**
 * Privacy, retention, capture toggles, redaction tester, pause-all control,
 * and destructive thread deletion.
 */
export function PrivacyScreen({
  projects,
  threads = [],
  recordingThread = null,
  onChanged = () => {},
}: PrivacyScreenProps) {
  const [defaultExclusions, setDefaultExclusions] = useState<string[]>([]);
  const [selectedProjectId, setSelectedProjectId] = useState<string>(projects[0]?.id ?? "");
  const [policy, setPolicy] = useState<ProjectPolicy | null>(null);
  const [exclusionsText, setExclusionsText] = useState("");
  const [patternsText, setPatternsText] = useState("");
  const [retentionDays, setRetentionDays] = useState("14");
  const [captureGit, setCaptureGit] = useState(true);
  const [captureCommands, setCaptureCommands] = useState(true);
  const [captureFileMetadata, setCaptureFileMetadata] = useState(true);

  const [sampleText, setSampleText] = useState("Authorization: Bearer nbf_demo_7F4A91C2_DO_NOT_SEND");
  const [testResult, setTestResult] = useState<RedactionTestResult | null>(null);

  const [busy, setBusy] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const confirmHeading = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    projectsApi
      .defaultExclusions()
      .then(setDefaultExclusions)
      .catch(() => setDefaultExclusions([]));
  }, []);

  useEffect(() => {
    if (!selectedProjectId && projects.length > 0) {
      setSelectedProjectId(projects[0].id);
    }
  }, [projects, selectedProjectId]);

  useEffect(() => {
    if (!selectedProjectId) {
      setPolicy(null);
      return;
    }
    projectsApi
      .getPolicy(selectedProjectId)
      .then((loaded) => {
        setPolicy(loaded);
        setRetentionDays(String(loaded.retentionDays));
        setCaptureGit(loaded.captureGit);
        setCaptureCommands(loaded.captureCommands);
        setCaptureFileMetadata(loaded.captureFileMetadata);
        setExclusionsText(loaded.exclusions.join("\n"));
        setPatternsText(loaded.redactionPatterns.join("\n"));
      })
      .catch((failure) => setError(toDomainError(failure).message));
  }, [selectedProjectId]);

  useEffect(() => {
    if (confirmDeleteId) confirmHeading.current?.focus();
  }, [confirmDeleteId]);

  const pauseAll = async () => {
    setBusy("pause-all");
    setError(null);
    setStatus(null);
    try {
      const paused = await threadsApi.pauseAll();
      setStatus(
        paused.length === 0
          ? "No active threads were recording."
          : `Paused ${pluralize(paused.length, "active thread")}. Nothing is being recorded.`,
      );
      await onChanged();
    } catch (failure) {
      setError(toDomainError(failure).message);
    } finally {
      setBusy(null);
    }
  };

  const savePolicy = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!selectedProjectId) return;
    setBusy("save-policy");
    setError(null);
    setStatus(null);
    try {
      const days = Number.parseInt(retentionDays, 10);
      if (!Number.isFinite(days)) {
        setError("Retention must be between 1 and 365 days.");
        return;
      }
      const next: ProjectPolicy = {
        projectId: selectedProjectId,
        retentionDays: days,
        captureGit,
        captureCommands,
        captureFileMetadata,
        exclusions: linesToArray(exclusionsText),
        redactionPatterns: linesToArray(patternsText),
      };
      const saved = await projectsApi.updatePolicy(next);
      setPolicy(saved);
      setStatus("Saved project privacy policy and applied retention rules.");
    } catch (failure) {
      setError(toDomainError(failure).message);
    } finally {
      setBusy(null);
    }
  };

  const runRedactionTest = async () => {
    setBusy("test-redaction");
    setError(null);
    try {
      const result = await privacyApi.testRedaction({
        projectId: selectedProjectId || null,
        customPatterns: linesToArray(patternsText),
        text: sampleText,
      });
      setTestResult(result);
    } catch (failure) {
      setError(toDomainError(failure).message);
    } finally {
      setBusy(null);
    }
  };

  const deleteThread = async (threadId: string) => {
    setBusy("delete-thread");
    setError(null);
    setStatus(null);
    try {
      const receipt = await threadsApi.delete(threadId);
      setConfirmDeleteId(null);
      setStatus(
        `Deleted thread and removed ${pluralize(receipt.eventsDeleted, "event")} and ${pluralize(receipt.checkpointsDeleted, "checkpoint")}.`,
      );
      await onChanged();
    } catch (failure) {
      setError(toDomainError(failure).message);
    } finally {
      setBusy(null);
    }
  };

  const disabled = busy !== null;

  return (
    <>
      <h1 id="screen-title" tabIndex={-1}>
        Privacy
      </h1>
      <p className="lede">What ThreadBack can see, what stays local, and how long raw evidence is kept.</p>

      {status ? (
        <p className="hint" role="status">
          <span aria-hidden="true">✓ </span>
          {status}
        </p>
      ) : null}
      {error ? (
        <p className="form-error" role="alert">
          <span aria-hidden="true">⚠ </span>
          {error}
        </p>
      ) : null}

      <section className="panel" aria-labelledby="pause-all-title">
        <div className="section-head">
          <h2 id="pause-all-title">Recording status</h2>
          <button
            type="button"
            disabled={disabled || !recordingThread}
            onClick={() => void pauseAll()}
          >
            {busy === "pause-all" ? "Pausing…" : "Pause all active capture"}
          </button>
        </div>
        <p className="hint">
          {recordingThread ? (
            <>
              <span aria-hidden="true">● </span>
              <strong>Recording active:</strong> {recordingThread.title}
            </>
          ) : (
            <>
              <span aria-hidden="true">○ </span>
              <strong>Not recording:</strong> No thread is currently collecting evidence.
            </>
          )}
        </p>
      </section>

      <section className="panel" aria-labelledby="boundary-title">
        <h2 id="boundary-title">Collection boundary</h2>
        <ul className="plain">
          <li>Evidence is recorded only while a thread is active, and only inside an approved folder.</li>
          <li>Paths are resolved first; links that point outside the folder are refused.</li>
          <li>ThreadBack never records screens, keystrokes, or global shell history.</li>
        </ul>
      </section>

      <section aria-labelledby="roots-title">
        <h2 id="roots-title">Approved folders</h2>
        {projects.length === 0 ? (
          <p className="empty">No folders approved.</p>
        ) : (
          <ul className="plain">
            {projects.map((project) => (
              <li key={project.id}>
                <strong>{project.displayName}</strong> <span className="path">{project.canonicalRoot}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="panel" aria-labelledby="rules-title">
        <h2 id="rules-title">Never collected (default exclusions)</h2>
        <ul className="chips">
          {defaultExclusions.map((rule) => (
            <li key={rule}>{rule}</li>
          ))}
        </ul>
      </section>

      {projects.length > 0 ? (
        <section className="panel" aria-labelledby="policy-title">
          <h2 id="policy-title">Project privacy &amp; retention policy</h2>
          <form onSubmit={(event) => void savePolicy(event)} noValidate>
            {projects.length > 1 ? (
              <Field id="policy-project" label="Project" required>
                {(aria) => (
                  <select
                    {...aria}
                    value={selectedProjectId}
                    onChange={(event) => setSelectedProjectId(event.target.value)}
                  >
                    {projects.map((project) => (
                      <option key={project.id} value={project.id}>
                        {project.displayName} ({project.canonicalRoot})
                      </option>
                    ))}
                  </select>
                )}
              </Field>
            ) : null}

            <Field
              id="retention-days"
              label="Raw event retention (days)"
              required
              hint="Default is 14 days (1–365). Expired raw events are removed while approved capsules stay intact."
            >
              {(aria) => (
                <input
                  {...aria}
                  type="number"
                  min={1}
                  max={365}
                  value={retentionDays}
                  onChange={(event) => setRetentionDays(event.target.value)}
                />
              )}
            </Field>

            <fieldset className="field">
              <legend>Allowed collectors</legend>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={captureGit}
                  onChange={(event) => setCaptureGit(event.target.checked)}
                />
                <span>Capture Git snapshots (branch, HEAD, dirty status, and changed paths)</span>
              </label>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={captureCommands}
                  onChange={(event) => setCaptureCommands(event.target.checked)}
                />
                <span>Capture wrapped commands (<code>tb run -- &lt;cmd&gt;</code>)</span>
              </label>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={captureFileMetadata}
                  onChange={(event) => setCaptureFileMetadata(event.target.checked)}
                />
                <span>Capture file-change metadata inside the project root</span>
              </label>
            </fieldset>

            <Field
              id="custom-exclusions"
              label="Additional excluded path globs (one per line)"
              hint="Relative to the project folder, e.g. fixtures/sensitive/**"
            >
              {(aria) => (
                <textarea
                  {...aria}
                  rows={3}
                  value={exclusionsText}
                  onChange={(event) => setExclusionsText(event.target.value)}
                />
              )}
            </Field>

            <Field
              id="custom-redaction-patterns"
              label="Custom redaction patterns (regular expressions, one per line)"
              hint="Matches are replaced with [REDACTED:custom] and block outbound payloads until excluded."
            >
              {(aria) => (
                <textarea
                  {...aria}
                  rows={3}
                  value={patternsText}
                  onChange={(event) => setPatternsText(event.target.value)}
                />
              )}
            </Field>

            <div className="actions">
              <button type="submit" className="primary" disabled={disabled || !policy}>
                {busy === "save-policy" ? "Saving…" : "Save privacy policy"}
              </button>
            </div>
          </form>
        </section>
      ) : null}

      <section className="panel" aria-labelledby="redaction-test-title">
        <h2 id="redaction-test-title">Test redaction rules</h2>
        <p className="hint">
          Paste a sample line to verify how built-in and custom patterns redact secrets before anything is stored or sent.
          Nothing you type here is saved.
        </p>
        <Field id="redaction-sample" label="Sample text to test">
          {(aria) => (
            <textarea
              {...aria}
              rows={3}
              value={sampleText}
              onChange={(event) => setSampleText(event.target.value)}
            />
          )}
        </Field>
        <div className="actions">
          <button type="button" disabled={disabled} onClick={() => void runRedactionTest()}>
            {busy === "test-redaction" ? "Testing…" : "Run redaction test"}
          </button>
        </div>
        {testResult ? (
          <div role="region" aria-label="Redaction test output">
            <p>
              <span aria-hidden="true">{testResult.hasSecret ? "⚠ " : "✓ "}</span>
              <strong>
                {testResult.hasSecret
                  ? `${pluralize(testResult.secretCount, "credential")} redacted`
                  : "No credentials found"}
              </strong>
            </p>
            <pre className="preformatted">{testResult.redactedText}</pre>
            {testResult.findings.length > 0 ? (
              <ul className="plain">
                {testResult.findings.map((finding) => (
                  <li key={finding.label}>
                    <code>[REDACTED:{finding.label}]</code> — {finding.description} ({finding.count}×)
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        ) : null}
      </section>

      {threads.length > 0 ? (
        <section className="panel" aria-labelledby="delete-threads-title">
          <h2 id="delete-threads-title">Delete stored threads</h2>
          <p className="hint">
            Deleting a thread removes its events, checkpoint drafts, approved capsules, corrections, and exports.
          </p>
          <ul className="plain">
            {threads.map((thread) => {
              const display = STATE_DISPLAY[thread.state];
              const confirming = confirmDeleteId === thread.id;
              return (
                <li key={thread.id}>
                  <div>
                    <strong>{thread.title}</strong>{" "}
                    <span className="hint">
                      ({display.glyph} {display.label})
                    </span>
                  </div>
                  {confirming ? (
                    <div role="group" aria-labelledby={`confirm-del-${thread.id}`}>
                      <h3 id={`confirm-del-${thread.id}`} tabIndex={-1} ref={confirmHeading}>
                        Permanently delete "{thread.title}"?
                      </h3>
                      <div className="actions">
                        <button
                          type="button"
                          className="danger"
                          disabled={disabled}
                          onClick={() => void deleteThread(thread.id)}
                        >
                          {busy === "delete-thread" ? "Deleting…" : "Confirm permanent deletion"}
                        </button>
                        <button
                          type="button"
                          disabled={disabled}
                          onClick={() => setConfirmDeleteId(null)}
                        >
                          Cancel
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className="actions">
                      <button
                        type="button"
                        disabled={disabled}
                        onClick={() => setConfirmDeleteId(thread.id)}
                      >
                        Delete "{thread.title}"…
                      </button>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </section>
      ) : null}
    </>
  );
}
