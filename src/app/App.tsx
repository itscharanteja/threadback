import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppStatus, Project, WorkThread } from "../bindings/contracts";
import { RecoveryBanner } from "../components/RecoveryBanner";
import { StatusBar } from "../components/StatusBar";
import { ApproveProjectScreen } from "../screens/ApproveProjectScreen";
import { HomeScreen } from "../screens/HomeScreen";
import { NewThreadScreen } from "../screens/NewThreadScreen";
import { PrivacyScreen } from "../screens/PrivacyScreen";
import { ThreadScreen } from "../screens/ThreadScreen";
import { useAppData, useOnline } from "../state/useAppData";

type View =
  | { name: "home" }
  | { name: "approve" }
  | { name: "new-thread"; projectId?: string }
  | { name: "thread"; threadId: string }
  | { name: "privacy" };

export function App() {
  const data = useAppData();
  const online = useOnline();
  const [view, setView] = useState<View>({ name: "home" });
  const [coreReady, setCoreReady] = useState(false);
  const [dismissed, setDismissed] = useState<string[]>([]);

  useEffect(() => {
    invoke<AppStatus>("app_status")
      .then((status) => setCoreReady(status.ready))
      .catch(() => setCoreReady(false));
  }, []);

  // Move focus to the new screen's heading so keyboard and screen-reader users
  // start at the top of what just opened.
  useEffect(() => {
    document.getElementById("screen-title")?.focus();
  }, [view, data.loaded]);

  const openThread = (thread: WorkThread) => setView({ name: "thread", threadId: thread.id });
  const shownThread =
    view.name === "thread" ? data.threads.find((thread) => thread.id === view.threadId) : undefined;
  const navTarget = data.recordingThread ?? data.threads[0];

  async function approved(project: Project) {
    await data.refresh();
    setView({ name: "new-thread", projectId: project.id });
  }

  async function started(thread: WorkThread) {
    await data.refresh();
    openThread(thread);
  }

  return (
    <>
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <header className="app-header">
        <span className="brand">ThreadBack</span>
        <nav aria-label="Main">
          <button
            type="button"
            aria-current={view.name === "home" ? "page" : undefined}
            onClick={() => setView({ name: "home" })}
          >
            Home
          </button>
          <button
            type="button"
            aria-current={view.name === "thread" ? "page" : undefined}
            disabled={!navTarget}
            onClick={() => navTarget && openThread(navTarget)}
          >
            {data.recordingThread ? "Active thread" : "Latest thread"}
          </button>
          <button
            type="button"
            aria-current={view.name === "privacy" ? "page" : undefined}
            onClick={() => setView({ name: "privacy" })}
          >
            Privacy
          </button>
        </nav>
      </header>

      <StatusBar
        coreReady={coreReady}
        online={online}
        recordingThread={data.recordingThread}
        onOpenThread={openThread}
      />
      <RecoveryBanner
        reports={data.recovery.filter((report) => !dismissed.includes(report.threadId))}
        onDismiss={(threadId) => setDismissed((current) => [...current, threadId])}
      />

      <main id="main" className="screen">
        {view.name === "home" && (
          <HomeScreen
            projects={data.projects}
            threads={data.threads}
            loadError={data.loadError}
            onApproveProject={() => setView({ name: "approve" })}
            onStartThread={(project) => setView({ name: "new-thread", projectId: project.id })}
            onOpenThread={openThread}
          />
        )}
        {view.name === "approve" && (
          <ApproveProjectScreen onApproved={approved} onCancel={() => setView({ name: "home" })} />
        )}
        {view.name === "new-thread" && (
          <NewThreadScreen
            projects={data.projects}
            initialProjectId={view.projectId}
            recordingTitle={data.recordingThread?.title}
            onStarted={started}
            onApproveProject={() => setView({ name: "approve" })}
          />
        )}
        {view.name === "thread" &&
          (shownThread ? (
            <ThreadScreen
              thread={shownThread}
              project={data.projects.find((project) => project.id === shownThread.projectId)}
              onChanged={data.refresh}
            />
          ) : (
            <p className="empty" role="status">
              {data.loaded ? "That thread no longer exists." : "Loading…"}
            </p>
          ))}
        {view.name === "privacy" && <PrivacyScreen projects={data.projects} />}
      </main>
    </>
  );
}
