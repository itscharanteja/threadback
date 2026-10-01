import { useCallback, useEffect, useState } from "react";
import type { Project, ThreadEvent, WorkThread } from "../bindings/contracts";
import { events as eventsApi } from "../api/commands";
import { toDomainError } from "../api/errors";
import { Timeline } from "../components/Timeline";
import { STATE_DISPLAY, formatTime, pluralize } from "../lib/format";

const MAX_PAGES = 50;

interface ThreadScreenProps {
  thread: WorkThread;
  project: Project | undefined;
}

/** A thread's goal, state, and evidence. Controls arrive with the active-thread workflow. */
export function ThreadScreen({ thread, project }: ThreadScreenProps) {
  const [events, setEvents] = useState<ThreadEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const all: ThreadEvent[] = [];
      let cursor: string | undefined;
      for (let page = 0; page < MAX_PAGES; page += 1) {
        const result = await eventsApi.list(thread.id, cursor);
        all.push(...result.events);
        if (!result.nextCursor) break;
        cursor = result.nextCursor;
      }
      setEvents(all);
      setError(null);
    } catch (failure) {
      setError(toDomainError(failure).message);
    }
  }, [thread.id]);

  useEffect(() => {
    setEvents(null);
    void load();
  }, [load]);

  const state = STATE_DISPLAY[thread.state];

  return (
    <>
      <h1 id="screen-title" tabIndex={-1}>
        {thread.title}
      </h1>
      <p className="state">
        <span aria-hidden="true">{state.glyph} </span>
        {state.label}
      </p>

      <section className="panel" aria-labelledby="goal-title">
        <h2 id="goal-title">Goal</h2>
        <p className="preformatted">{thread.goal}</p>
        <p className="hint">
          {project ? `${project.displayName} · ` : ""}Started {formatTime(thread.startedAt)}
        </p>
      </section>

      <section aria-labelledby="timeline-title">
        <div className="section-head">
          <h2 id="timeline-title">Evidence</h2>
          <button type="button" onClick={() => void load()}>
            Refresh
          </button>
        </div>
        {error ? (
          <p className="form-error" role="alert">
            <span aria-hidden="true">⚠ </span>
            {error}
          </p>
        ) : events === null ? (
          <p className="empty" role="status">
            Loading evidence…
          </p>
        ) : (
          <>
            <p className="hint" role="status">
              {pluralize(events.length, "event")} recorded.
            </p>
            <Timeline events={events} />
          </>
        )}
      </section>
    </>
  );
}
