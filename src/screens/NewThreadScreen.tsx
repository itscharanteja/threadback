import { useState, type FormEvent } from "react";
import type { Project, ProjectId, WorkThread } from "../bindings/contracts";
import { threads } from "../api/commands";
import { Field } from "../components/Field";
import { useFieldErrors } from "../lib/useFieldErrors";

const FIELDS = ["projectId", "title", "goal", "hypothesis"];

interface NewThreadScreenProps {
  projects: Project[];
  initialProjectId?: ProjectId;
  /** A thread that is already recording blocks starting another. */
  recordingTitle?: string;
  onStarted: (thread: WorkThread) => void;
  onApproveProject: () => void;
}

export function NewThreadScreen({
  projects,
  initialProjectId,
  recordingTitle,
  onStarted,
  onApproveProject,
}: NewThreadScreenProps) {
  const [projectId, setProjectId] = useState<string>(initialProjectId ?? projects[0]?.id ?? "");
  const [title, setTitle] = useState("");
  const [goal, setGoal] = useState("");
  const [hypothesis, setHypothesis] = useState("");
  const [busy, setBusy] = useState(false);
  const { errors, formError, fail, failFields, clear } = useFieldErrors(FIELDS);

  if (projects.length === 0) {
    return (
      <>
        <h1 id="screen-title" tabIndex={-1}>
          Start a thread
        </h1>
        <p className="lede">A thread belongs to an approved project, and you have none yet.</p>
        <button type="button" className="primary" onClick={onApproveProject}>
          Approve a project folder
        </button>
      </>
    );
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const problems: Record<string, string> = {};
    if (!projectId) problems.projectId = "Choose a project.";
    if (!title.trim()) problems.title = "Give the thread a title.";
    if (!goal.trim()) problems.goal = "Describe the outcome you are working toward.";
    if (Object.keys(problems).length > 0) {
      failFields(problems);
      return;
    }
    setBusy(true);
    clear();
    try {
      onStarted(
        await threads.start({
          projectId,
          title,
          goal,
          hypothesis: hypothesis.trim() ? hypothesis : null,
        }),
      );
    } catch (error) {
      fail(error);
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} noValidate aria-labelledby="screen-title">
      <h1 id="screen-title" tabIndex={-1}>
        Start a thread
      </h1>
      <p className="lede">
        State what you are trying to accomplish. ThreadBack records a first Git snapshot when you
        start.
      </p>

      {recordingTitle ? (
        <p className="form-error" role="status">
          <span aria-hidden="true">⚠ </span>
          “{recordingTitle}” is still recording. Pause or end it before starting another thread.
        </p>
      ) : null}
      {formError ? (
        <p className="form-error" id="form-error" role="alert" tabIndex={-1}>
          <span aria-hidden="true">⚠ </span>
          {formError}
        </p>
      ) : null}

      <Field id="projectId" label="Project" required error={errors.projectId}>
        {(aria) => (
          <select {...aria} value={projectId} onChange={(event) => setProjectId(event.target.value)}>
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.displayName}
              </option>
            ))}
          </select>
        )}
      </Field>

      <Field id="title" label="Title" required error={errors.title}>
        {(aria) => (
          <input
            {...aria}
            type="text"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            placeholder="Duplicate capture investigation"
          />
        )}
      </Field>

      <Field
        id="goal"
        label="Intended outcome"
        required
        error={errors.goal}
        hint="The goal stays visible on the thread until you edit it."
      >
        {(aria) => (
          <textarea
            {...aria}
            rows={3}
            value={goal}
            onChange={(event) => setGoal(event.target.value)}
            placeholder="Find why retries create a second provider capture"
          />
        )}
      </Field>

      <Field id="hypothesis" label="Starting hypothesis" error={errors.hypothesis}>
        {(aria) => (
          <textarea
            {...aria}
            rows={2}
            value={hypothesis}
            onChange={(event) => setHypothesis(event.target.value)}
          />
        )}
      </Field>

      <div className="actions">
        <button type="submit" className="primary" disabled={busy} aria-busy={busy}>
          {busy ? "Starting…" : "Start thread"}
        </button>
      </div>
    </form>
  );
}
