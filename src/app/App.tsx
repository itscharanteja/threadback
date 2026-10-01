import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppStatus } from "../bindings/contracts";

export function App() {
  const [status, setStatus] = useState<AppStatus>({
    service: "threadback",
    version: "0.1.0",
    ready: false,
  });

  useEffect(() => {
    invoke<AppStatus>("app_status").then(setStatus).catch(() => {
      // Browser-only Vite previews do not provide the Tauri command bridge.
    });
  }, []);

  return (
    <main className="shell">
      <p className="eyebrow">Private interruption recovery</p>
      <h1>ThreadBack</h1>
      <p className="promise">
        Preserve the reasoning state behind unfinished work, then return to one
        evidence-backed next action.
      </p>
      <section className="status" aria-label="Application status">
        <span className={status.ready ? "dot ready" : "dot"} aria-hidden="true" />
        <span>{status.ready ? "Local core ready" : "Starting local core"}</span>
      </section>
    </main>
  );
}
