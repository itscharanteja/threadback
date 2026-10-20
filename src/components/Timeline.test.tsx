import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ThreadEvent } from "../bindings/contracts";
import { entryItems, eventItems } from "../lib/format";
import { Timeline } from "./Timeline";

const excerpt = (over: Record<string, unknown>) => ({
  head: "", tail: "", originalBytes: 0, omittedBytes: 0, truncated: false, binary: false, redactions: 0, ...over,
});

const finished = (stdout: Record<string, unknown>, exitCode = 1): ThreadEvent => ({
  id: "e1",
  threadId: "t1",
  timestamp: "2026-10-01T10:00:00Z",
  sensitivity: "normal",
  contentHash: "h",
  payload: {
    type: "command_finished",
    payload: { startedEventId: "s", exitCode, signal: null, durationMs: 4200, stdout: excerpt(stdout), stderr: excerpt({}) },
  },
});

describe("Timeline command output", () => {
  it("says in words, without opening anything, that output was truncated", () => {
    render(
      <Timeline
        items={eventItems([
          finished({ head: "retry trace 00001", tail: "retry trace 20000", originalBytes: 1_060_000, omittedBytes: 1_047_000, truncated: true }),
        ])}
      />,
    );
    const output = screen.getByText("Standard output").closest("summary")!;
    expect(output).toHaveTextContent("Showing the first and last parts of 1.0 MB; 1022.5 KB omitted.");
    expect(screen.getByText("failed (exit code 1) after 4.20 s")).toBeInTheDocument();
  });

  it("shows the omitted gap between the head and the tail", () => {
    render(
      <Timeline
        items={eventItems([
          finished({ head: "first line", tail: "FAIL last line", originalBytes: 5_000, omittedBytes: 4_000, truncated: true }),
        ])}
      />,
    );
    const details = screen.getByText("Standard output").closest("details")!;
    expect(within(details).getByText(/first line[\s\S]*3\.9 KB omitted[\s\S]*FAIL last line/)).toBeInTheDocument();
  });

  it("does not claim truncation when everything was kept", () => {
    render(<Timeline items={eventItems([finished({ head: "all of it", originalBytes: 9 }, 0)])} />);
    expect(screen.queryByText(/omitted/)).toBeNull();
    expect(screen.getByText("succeeded (exit code 0) after 4.20 s")).toBeInTheDocument();
  });

  it("marks an event whose credential was redacted, in words", () => {
    const note: ThreadEvent = {
      id: "n1",
      threadId: "t1",
      timestamp: "2026-10-01T10:00:00Z",
      sensitivity: "sensitive",
      contentHash: "h",
      payload: { type: "note_added", payload: { text: "401 with Bearer [REDACTED:bearer-token]", label: "fact" } },
    };
    render(<Timeline items={eventItems([note])} />);
    expect(screen.getByText(/Credential redacted/)).toBeInTheDocument();
    expect(screen.getByText(/Bearer \[REDACTED:bearer-token\]/)).toBeInTheDocument();
  });

  it("does not mark ordinary events", () => {
    render(<Timeline items={eventItems([finished({ head: "ok", originalBytes: 2 }, 0)])} />);
    expect(screen.queryByText(/Credential redacted/)).toBeNull();
  });

  it("says how many credentials were replaced in command output", () => {
    const event = finished({ head: "Bearer [REDACTED:bearer-token]\n", originalBytes: 40, redactions: 2 });
    render(<Timeline items={eventItems([{ ...event, sensitivity: "sensitive" }])} />);
    const summary = screen.getByText("Standard output").closest("summary")!;
    expect(summary).toHaveTextContent("2 credentials replaced with [REDACTED:…] before storing.");
    expect(screen.getByText(/Credential redacted/)).toBeInTheDocument();
  });

  it("flags a checkpoint entry that contains a redaction marker", () => {
    const entry = {
      eventId: "e1",
      timestamp: "2026-10-01T10:00:00Z",
      kind: "note.added",
      content: { label: "fact", text: "saw [REDACTED:api-token]" },
    };
    render(<Timeline items={entryItems([entry])} />);
    expect(screen.getByText(/Credential redacted/)).toBeInTheDocument();
  });
});

describe("Timeline date organization and time highlighting", () => {
  it("places dates on the left side and highlights recorded times", () => {
    const e1: ThreadEvent = {
      id: "ev1",
      threadId: "t1",
      timestamp: "2026-10-08T09:30:00Z",
      sensitivity: "normal",
      contentHash: "h1",
      payload: { type: "note_added", payload: { text: "First observation", label: "fact" } },
    };
    const e2: ThreadEvent = {
      id: "ev2",
      threadId: "t1",
      timestamp: "2026-10-08T09:35:00Z",
      sensitivity: "normal",
      contentHash: "h2",
      payload: { type: "note_added", payload: { text: "Second observation", label: "hypothesis" } },
    };

    const { container } = render(<Timeline items={eventItems([e1, e2])} />);

    // Respective date is rendered in the left date badge
    const dateLabels = container.querySelectorAll(".timeline-date-label");
    expect(dateLabels).toHaveLength(1);
    expect(container.querySelector(".timeline-date-sub")).toHaveTextContent("2 events");

    // Both items are stacked under this date, reversed (latest record first, older last)
    const items = container.querySelectorAll(".timeline-item");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("Second observation");
    expect(items[1]).toHaveTextContent("First observation");

    // Times are highlighted with timeline-time-highlight badge
    const timeBadges = container.querySelectorAll(".timeline-time-highlight");
    expect(timeBadges).toHaveLength(2);
    for (const badge of timeBadges) {
      expect(badge).toHaveClass("timeline-time-highlight");
    }
  });

  it("stacks items across multiple dates in reverse order with the latest date first", () => {
    const day1: ThreadEvent = {
      id: "d1",
      threadId: "t1",
      timestamp: "2026-10-08T10:00:00Z",
      sensitivity: "normal",
      contentHash: "h1",
      payload: { type: "note_added", payload: { text: "Day 1 note", label: "fact" } },
    };
    const day2: ThreadEvent = {
      id: "d2",
      threadId: "t1",
      timestamp: "2026-10-09T14:00:00Z",
      sensitivity: "normal",
      contentHash: "h2",
      payload: { type: "note_added", payload: { text: "Day 2 note", label: "question" } },
    };

    const { container } = render(<Timeline items={eventItems([day1, day2])} />);

    // Latest date (day2) comes first, older date (day1) comes last
    const groups = container.querySelectorAll(".timeline-date-group");
    expect(groups).toHaveLength(2);
    expect(groups[0]).toHaveTextContent("Day 2 note");
    expect(groups[1]).toHaveTextContent("Day 1 note");

    const dateLabels = container.querySelectorAll(".timeline-date-label");
    expect(dateLabels).toHaveLength(2);
    expect(dateLabels[0].textContent).not.toEqual(dateLabels[1].textContent);
  });
});
