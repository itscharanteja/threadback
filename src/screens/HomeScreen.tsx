import type { Project, WorkThread } from "../bindings/contracts";
import { STATE_DISPLAY, formatTime } from "../lib/format";

interface HomeScreenProps {
  projects: Project[];
  threads: WorkThread[];
  loadError: string | null;
  onApproveProject: () => void;
  onStartThread: (project: Project) => void;
  onOpenThread: (thread: WorkThread) => void;
}

export function HomeScreen({
  projects,
  threads,
  loadError,
  onApproveProject,
  onStartThread,
  onOpenThread,
}: HomeScreenProps) {
  const projectName = (thread: WorkThread) =>
    projects.find((project) => project.id === thread.projectId)?.displayName ?? "Unknown project";

  return (
    <>
      <h1 id="screen-title" tabIndex={-1}>
        ThreadBack
      </h1>
      <p className="lede">
        Preserve the reasoning behind unfinished work, then return to one evidence-backed next action.
      </p>

      {loadError ? (
        <p className="form-error" role="alert">
          <span aria-hidden="true">⚠ </span>
          Local data is unavailable: {loadError}
        </p>
      ) : null}

      <section aria-labelledby="projects-title">
        <div className="section-head">
          <h2 id="projects-title">Approved projects</h2>
          <button type="button" onClick={onApproveProject}>
            Approve a project…
          </button>
        </div>
        {projects.length === 0 ? (
          <p className="empty">
            No projects yet. Approve a Git repository to start your first thread.
          </p>
        ) : (
          <ul className="cards">
            {projects.map((project) => (
              <li key={project.id} className="card">
                <div>
                  <h3>{project.displayName}</h3>
                  <p className="hint path">{project.canonicalRoot}</p>
                </div>
                <button type="button" className="primary" onClick={() => onStartThread(project)}>
                  Start thread
                  <span className="visually-hidden"> in {project.displayName}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="threads-title">
        <h2 id="threads-title">Recent threads</h2>
        {threads.length === 0 ? (
          <p className="empty">No threads yet.</p>
        ) : (
          <ul className="cards">
            {threads.map((thread) => {
              const state = STATE_DISPLAY[thread.state];
              return (
                <li key={thread.id} className="card">
                  <div>
                    <h3>{thread.title}</h3>
                    <p className="hint">
                      <span aria-hidden="true">{state.glyph} </span>
                      {state.label} · {projectName(thread)} · updated {formatTime(thread.updatedAt)}
                    </p>
                  </div>
                  <button type="button" onClick={() => onOpenThread(thread)}>
                    Open
                    <span className="visually-hidden"> {thread.title}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </>
  );
}
