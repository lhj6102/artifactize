// Status and tone names to theme classes; the glyphs and words come from the backend.
import type { Status, Tone } from "../ipc/client";

const STATUS: Record<Status, Tone> = {
  ERROR: "error",
  RED: "red",
  BLOCKED: "blocked",
  RUNNING: "running",
  WAITING_HUMAN: "human",
  DEFERRED: "human",
  SIGNOFFS_PENDING: "human",
  PROVISIONAL: "muted",
  QUEUED: "queued",
  BUDGET_EXHAUSTED: "blocked",
  WAIT_DEPENDENCY: "muted",
  WAIT: "muted",
  STALE: "muted",
  UNREVIEWED: "muted",
  INCOMPLETE: "muted",
  GREEN: "green",
  BASIS: "muted",
};

/** The theme colour of a status, as a CSS value, for what a tone class cannot reach (a tint). */
export function toneColour(status: Status): string {
  return `var(--${STATUS[status]})`;
}

/** The theme class of a status, or of a tone. */
export function tone(value: Status | Tone | null | undefined): string {
  if (!value) return "";
  return `tone-${value in STATUS ? STATUS[value as Status] : value}`;
}
