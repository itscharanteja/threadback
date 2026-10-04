import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ThreadEvent } from "../bindings/contracts";
import { eventItems } from "../lib/format";
import { Timeline } from "./Timeline";

const excerpt = (over: Record<string, unknown>) => ({
  head: "", tail: "", originalBytes: 0, omittedBytes: 0, truncated: false, binary: false, ...over,
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
});
