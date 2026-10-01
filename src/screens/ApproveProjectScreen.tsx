import { useEffect, useState, type FormEvent } from "react";
import type { Project } from "../bindings/contracts";
import { chooseProjectFolder } from "../api/dialog";
import { projects } from "../api/commands";
import { Field } from "../components/Field";
import { folderName } from "../lib/format";
import { useFieldErrors } from "../lib/useFieldErrors";

const FIELDS = ["rootPath", "displayName", "consent"];

interface ApproveProjectScreenProps {
  onApproved: (project: Project) => void;
  onCancel: () => void;
}

/** Onboarding: choose a folder, review what is excluded, and approve it. */
export function ApproveProjectScreen({ onApproved, onCancel }: ApproveProjectScreenProps) {
  const [path, setPath] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [consent, setConsent] = useState(false);
  const [exclusions, setExclusions] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const { errors, formError, fail, failFields, clear } = useFieldErrors(FIELDS);

  useEffect(() => {
    projects.defaultExclusions().then(setExclusions).catch(() => setExclusions([]));
  }, []);

  async function pickFolder() {
    clear();
    try {
      const selection = await chooseProjectFolder();
      if (selection) {
        setPath(selection);
        setName((current) => current || folderName(selection));
      }
    } catch (error) {
      fail(error);
    }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const problems: Record<string, string> = {};
    if (!path) problems.rootPath = "Choose the project folder first.";
    if (!name.trim()) problems.displayName = "Give the project a name.";
    if (!consent) problems.consent = "Confirm that ThreadBack may collect evidence inside this folder.";
    if (Object.keys(problems).length > 0) {
      failFields(problems);
      return;
    }
    setBusy(true);
    clear();
    try {
      onApproved(await projects.create({ displayName: name, rootPath: path! }));
    } catch (error) {
      fail(error);
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} noValidate aria-labelledby="screen-title">
      <h1 id="screen-title" tabIndex={-1}>
        Approve a project folder
      </h1>
      <p className="lede">
        ThreadBack only looks inside folders you approve, and only while a thread is active. It must be
        a Git repository.
      </p>

      {formError ? (
        <p className="form-error" id="form-error" role="alert" tabIndex={-1}>
          <span aria-hidden="true">⚠ </span>
          {formError}
        </p>
      ) : null}

      <Field
        id="rootPath"
        label="Project folder"
        required
        error={errors.rootPath}
        hint={path ?? "No folder chosen yet."}
      >
        {(aria) => (
          <button type="button" {...aria} onClick={pickFolder}>
            {path ? "Choose a different folder…" : "Choose folder…"}
          </button>
        )}
      </Field>

      <Field id="displayName" label="Project name" required error={errors.displayName}>
        {(aria) => (
          <input {...aria} type="text" value={name} onChange={(event) => setName(event.target.value)} />
        )}
      </Field>

      <section className="panel" aria-labelledby="exclusions-title">
        <h2 id="exclusions-title">Never collected</h2>
        <p className="hint">
          These are skipped in every project: dependency folders, build output, Git internals, secret
          files, keys, and binaries.
        </p>
        <details>
          <summary>Show all {exclusions.length} exclusion rules</summary>
          <ul className="chips">
            {exclusions.map((rule) => (
              <li key={rule}>{rule}</li>
            ))}
          </ul>
        </details>
      </section>

      <section className="panel" aria-labelledby="privacy-title">
        <h2 id="privacy-title">Retention and outbound data</h2>
        <ul className="plain">
          <li>Raw evidence is kept for 14 days unless you change it in Privacy.</li>
          <li>Nothing leaves this computer until you review the exact payload and confirm it.</li>
          <li>The model connection and its API key are set up separately, in the Keychain.</li>
        </ul>
      </section>

      <Field id="consent" label="Consent" required error={errors.consent}>
        {(aria) => (
          <span className="check">
            <input
              {...aria}
              type="checkbox"
              checked={consent}
              onChange={(event) => setConsent(event.target.checked)}
            />
            <span>ThreadBack may collect evidence inside this folder while a thread is active.</span>
          </span>
        )}
      </Field>

      <div className="actions">
        <button type="submit" className="primary" disabled={busy} aria-busy={busy}>
          {busy ? "Approving…" : "Approve project"}
        </button>
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}
