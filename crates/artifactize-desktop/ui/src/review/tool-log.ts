// The Tool log of one review: each run's start, what it printed as it ran, and its end. The
// backend sends the lines as they arrive; this only files them under their run.
import type { LogLine } from "../ipc/client";

/** One line of a run in the Tool log: what it printed, red when it went to stderr. */
export type LogText = { text: string; error: boolean };

/** One tool run in the review's Tool log: its start line's facts, what it printed as it ran,
 * and its end line once it ended. A launched program's lines keep arriving after its tool
 * returned. */
export type LogEntry = {
  run: string;
  /** `HH:MM:SS` of its start, local time. */
  at: string;
  operation: string;
  artifact: string;
  lines: LogText[];
  end: LogText | null;
};

/** `HH:MM:SS` of `at` (milliseconds since the epoch), local time. */
function clock(at: number | null): string {
  return at === null ? "--:--:--" : new Date(at).toTimeString().slice(0, 8);
}

/** `log` with the library's `lines` taken in: a start opens an entry, the rest join their
 * run's entry. Lines of a run this log never saw start (cleared since) are dropped. */
export function logged(log: LogEntry[], lines: LogLine[]): LogEntry[] {
  const next = [...log];
  const copied = new Set<number>();
  for (const { run, line } of lines) {
    if (line.kind === "start") {
      next.push({ run, at: clock(line.at), operation: line.operation, artifact: line.artifact, lines: [], end: null });
      copied.add(next.length - 1);
      continue;
    }
    const at = next.findLastIndex((entry) => entry.run === run);
    if (at < 0) continue;
    // Each touched entry is copied once per batch, then appended to in place.
    if (!copied.has(at)) {
      next[at] = { ...next[at], lines: [...next[at].lines] };
      copied.add(at);
    }
    if (line.kind === "end") next[at].end = { text: line.text, error: line.error };
    else next[at].lines.push({ text: line.text, error: line.kind === "stderr" });
  }
  return next;
}
