import { useEffect, useState } from "react";
import type { Project } from "../bindings/contracts";
import { projects as projectsApi } from "../api/commands";

/** Read-only for now; retention, capture toggles, and deletion arrive with the privacy controls. */
export function PrivacyScreen({ projects }: { projects: Project[] }) {
  const [exclusions, setExclusions] = useState<string[]>([]);
  useEffect(() => {
    projectsApi.defaultExclusions().then(setExclusions).catch(() => setExclusions([]));
  }, []);

  return (
    <>
      <h1 id="screen-title" tabIndex={-1}>
        Privacy
      </h1>
      <p className="lede">What ThreadBack can see, and when.</p>

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
        <h2 id="rules-title">Never collected</h2>
        <ul className="chips">
          {exclusions.map((rule) => (
            <li key={rule}>{rule}</li>
          ))}
        </ul>
      </section>
    </>
  );
}
