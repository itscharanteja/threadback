import { useCallback, useEffect, useState } from "react";
import type { Project, RecoveryReport, WorkThread } from "../bindings/contracts";
import { projects as projectsApi, recovery, threads as threadsApi } from "../api/commands";
import { toDomainError } from "../api/errors";
import { isRecording } from "../lib/format";

export interface AppData {
  loaded: boolean;
  /** Set when the local core cannot be reached, e.g. in a browser-only preview. */
  loadError: string | null;
  projects: Project[];
  threads: WorkThread[];
  recovery: RecoveryReport[];
  recordingThread: WorkThread | undefined;
  refresh: () => Promise<void>;
}

export function useAppData(): AppData {
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [threads, setThreads] = useState<WorkThread[]>([]);
  const [reports, setReports] = useState<RecoveryReport[]>([]);

  const refresh = useCallback(async () => {
    try {
      const [nextProjects, nextThreads] = await Promise.all([projectsApi.list(), threadsApi.list()]);
      setProjects(nextProjects);
      setThreads(nextThreads);
      setLoadError(null);
    } catch (error) {
      setLoadError(toDomainError(error).message);
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
    recovery.reports().then(setReports).catch(() => setReports([]));
  }, [refresh]);

  return {
    loaded,
    loadError,
    projects,
    threads,
    recovery: reports,
    recordingThread: threads.find((thread) => isRecording(thread.state)),
    refresh,
  };
}

export function useOnline(): boolean {
  const [online, setOnline] = useState(() => navigator.onLine);
  useEffect(() => {
    const update = () => setOnline(navigator.onLine);
    window.addEventListener("online", update);
    window.addEventListener("offline", update);
    return () => {
      window.removeEventListener("online", update);
      window.removeEventListener("offline", update);
    };
  }, []);
  return online;
}
