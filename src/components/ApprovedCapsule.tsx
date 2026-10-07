import { type FormEvent, forwardRef, useEffect, useState } from "react";
import type { Checkpoint, Correction, JsonValue, OpenedArtifact, ProjectId, RepositoryDrift } from "../bindings/contracts";
import { checkpoints as checkpointsApi, exports as exportsApi, projects as projectsApi } from "../api/commands";
import { chooseProjectFolder } from "../api/dialog";
import { toDomainError } from "../api/errors";
import { formatTime, pluralize } from "../lib/format";
import { CapsuleView } from "./CapsuleView";

const show = (value: JsonValue): string => {
  if (value === null) return "(nothing)";
  return typeof value === "string" ? value : JSON.stringify(value);
};

interface ApprovedCapsuleProps {
  checkpoint: Checkpoint;
  /** Corrections already known, such as straight after approval. */
  corrections?: Correction[];
}

/**
 * The approved capsule, read-only, with repository drift comparison, explicit
 * copy/open/export controls, the changes the user made to the model's draft,
 * and the draft itself, which approval never touches.
 */
export const ApprovedCapsule = forwardRef<HTMLHeadingElement, ApprovedCapsuleProps>(function ApprovedCapsule(
  { checkpoint, corrections: known },
  headingRef,
) {
  const [corrections, setCorrections] = useState<Correction[] | null>(known ?? null);
  const [drift, setDrift] = useState<RepositoryDrift | null>(null);
  const [projectId, setProjectId] = useState<ProjectId | null>(null);
  const [relinkPath, setRelinkPath] = useState("");
  const [relinking, setRelinking] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [openedArtifact, setOpenedArtifact] = useState<OpenedArtifact | null>(null);
  const [showOriginal, setShowOriginal] = useState(false);
  const [exporting, setExporting] = useState<string | null>(null);

  useEffect(() => {
    setOpenedArtifact(null);
    setNotice(null);
    setFailure(null);
    checkpointsApi
      .getResume(checkpoint.id)
      .then((resume) => {
        setDrift(resume.drift);
        setProjectId(resume.thread.projectId);
        if (!known) setCorrections(resume.corrections);
      })
      .catch((error) => {
        setFailure(toDomainError(error).message);
        if (!known) {
          checkpointsApi
            .corrections(checkpoint.id)
            .then(setCorrections)
            .catch(() => {});
        }
      });
  }, [checkpoint.id, known]);

  const handleRelink = async (event: FormEvent) => {
    event.preventDefault();
    if (!projectId) return;
    setRelinking(true);
    setFailure(null);
    setNotice(null);
    try {
      const updated = await projectsApi.relink({ projectId, rootPath: relinkPath });
      const resume = await checkpointsApi.getResume(checkpoint.id);
      setDrift(resume.drift);
      setNotice(`Relinked project folder to ${updated.canonicalRoot}.`);
      setRelinkPath("");
    } catch (error) {
      setFailure(toDomainError(error).message);
    } finally {
      setRelinking(false);
    }
  };

  const browseRelinkFolder = async () => {
    const selected = await chooseProjectFolder();
    if (selected) setRelinkPath(selected);
  };

  const copyInstruction = async (instruction: string) => {
    setFailure(null);
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(instruction);
      }
      setNotice(`Copied next action to clipboard (not executed): ${instruction}`);
    } catch {
      setNotice(`Next action ready to copy: ${instruction}`);
    }
  };

  const openArtifact = async (relativePath: string) => {
    setFailure(null);
    try {
      const opened = await checkpointsApi.openArtifact(checkpoint.id, relativePath);
      setOpenedArtifact(opened);
      setNotice(
        opened.exists
          ? `Opened ${opened.relativePath}.`
          : `Artifact missing on disk; showing last known relative path (${opened.relativePath}).`,
      );
    } catch (error) {
      setFailure(toDomainError(error).message);
    }
  };

  const runExport = async (format: "json" | "markdown") => {
    setExporting(format);
    setFailure(null);
    try {
      const result =
        format === "json"
          ? await exportsApi.json(checkpoint.id)
          : await exportsApi.markdown(checkpoint.id);
      setNotice(
        `Exported approved capsule as ${result.format.toUpperCase()} to ${result.path} (${result.bytes} bytes).`,
      );
    } catch (error) {
      setFailure(toDomainError(error).message);
    } finally {
      setExporting(null);
    }
  };

  const isRepoUnavailable =
    drift?.signals.some((signal) => signal.kind === "repository_unavailable") ?? false;

  return (
    <>
      {drift ? (
        <section className="panel" aria-labelledby="drift-title">
          <h2 id="drift-title">Repository state since checkpoint</h2>
          <p>
            <span aria-hidden="true">{drift.hasDrift ? "⚠ " : "✓ "}</span>
            <strong>{drift.hasDrift ? "Drift detected: " : "No drift: "}</strong>
            {drift.summary}
          </p>
          {drift.signals.length > 0 ? (
            <ul className="plain" aria-label="Repository drift signals">
              {drift.signals.map((signal, index) => (
                <li key={`${signal.kind}-${index}`}>
                  <span aria-hidden="true">• </span>
                  {signal.label}
                </li>
              ))}
            </ul>
          ) : null}
          {isRepoUnavailable && projectId ? (
            <form onSubmit={(event) => void handleRelink(event)} noValidate>
              <div className="field">
                <label htmlFor="relink-root-path">Relink moved project folder</label>
                <p className="hint" id="relink-root-path-hint">
                  Your approved capsule is preserved. Choose the folder's new location to restore drift checks and file previews.
                </p>
                <div className="input-row">
                  <input
                    id="relink-root-path"
                    type="text"
                    aria-describedby="relink-root-path-hint"
                    value={relinkPath}
                    onChange={(event) => setRelinkPath(event.target.value)}
                    placeholder="/Users/you/code/payments"
                  />
                  <button type="button" onClick={() => void browseRelinkFolder()}>
                    Browse…
                  </button>
                </div>
              </div>
              <div className="actions">
                <button type="submit" className="primary" disabled={relinking}>
                  {relinking ? "Relinking…" : "Relink project folder"}
                </button>
              </div>
            </form>
          ) : null}
        </section>
      ) : null}

      {notice ? (
        <p className="hint" role="status">
          <span aria-hidden="true">✓ </span>
          {notice}
        </p>
      ) : null}
      {failure ? (
        <p className="form-error" role="alert">
          <span aria-hidden="true">⚠ </span>
          {failure}
        </p>
      ) : null}

      <CapsuleView
        ref={headingRef}
        checkpoint={checkpoint}
        capsule={checkpoint.approved}
        title="Approved resume capsule"
        idPrefix="approved"
        onCopyInstruction={copyInstruction}
        onOpenArtifact={openArtifact}
        intro={
          <p className="hint">
            Approved{checkpoint.approvedAt ? ` ${formatTime(checkpoint.approvedAt)}` : ""}. This is your version; the
            model's original draft is kept separately and was not changed. Nothing runs automatically.
          </p>
        }
      />

      {openedArtifact ? (
        <section className="panel" aria-labelledby="opened-artifact-title">
          <div className="section-head">
            <h2 id="opened-artifact-title">Opened file: {openedArtifact.relativePath}</h2>
            <button type="button" onClick={() => setOpenedArtifact(null)}>
              Close file
            </button>
          </div>
          <p className="hint">Path: <code>{openedArtifact.resolvedPath}</code></p>
          {openedArtifact.exists && openedArtifact.contentPreview !== null ? (
            <pre className="preformatted">{openedArtifact.contentPreview}</pre>
          ) : (
            <p className="empty">
              Artifact missing on disk. Last known relative path: <code>{openedArtifact.relativePath}</code>
            </p>
          )}
        </section>
      ) : null}

      <section className="panel" aria-labelledby="export-title">
        <h2 id="export-title">Export approved capsule</h2>
        <p className="hint">Exports contain only your approved capsule—never unapproved drafts or excluded evidence.</p>
        <div className="actions">
          <button
            type="button"
            disabled={exporting !== null}
            onClick={() => void runExport("json")}
          >
            {exporting === "json" ? "Exporting JSON…" : "Export JSON"}
          </button>
          <button
            type="button"
            disabled={exporting !== null}
            onClick={() => void runExport("markdown")}
          >
            {exporting === "markdown" ? "Exporting Markdown…" : "Export Markdown"}
          </button>
        </div>
      </section>

      <section className="panel" aria-labelledby="corrections-title">
        <h2 id="corrections-title">Your corrections</h2>
        {corrections === null ? (
          <p className="empty" role="status">
            Loading corrections…
          </p>
        ) : corrections.length === 0 ? (
          <p className="empty">None. You approved the model's draft as it was.</p>
        ) : (
          <>
            <p className="hint">{pluralize(corrections.length, "change")} recorded against the model's draft.</p>
            <ul className="plain" aria-label="Recorded corrections">
              {corrections.map((correction) => (
                <li key={correction.id}>
                  <code>{correction.fieldPath}</code>
                  <br />
                  <span className="hint">Before:</span> {show(correction.before)}
                  <br />
                  <span className="hint">After:</span> {show(correction.after)}
                </li>
              ))}
            </ul>
          </>
        )}
        <button type="button" aria-expanded={showOriginal} onClick={() => setShowOriginal(!showOriginal)}>
          {showOriginal ? "Hide the original model draft" : "Show the original model draft"}
        </button>
        {showOriginal ? (
          <CapsuleView
            checkpoint={checkpoint}
            capsule={checkpoint.draft}
            title="Original model draft"
            idPrefix="original"
            headingLevel={3}
            intro={<p className="hint">Exactly as the model returned it. It is read-only and never changes.</p>}
          />
        ) : null}
      </section>
    </>
  );
});
