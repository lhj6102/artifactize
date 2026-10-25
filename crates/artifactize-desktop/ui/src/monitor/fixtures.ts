// Snapshots as the backend sends them, for component tests against a mocked command boundary.
import type { BranchKey, BranchView, BrowseView, Chip, WaitingRow } from "../ipc/client";

/** A snapshot with nothing listed, at `revision`. */
export function browseView(revision: number, extra: Partial<BrowseView> = {}): BrowseView {
  return {
    query: { text: "sort:recent", error: null },
    branches: [],
    waiting: [],
    launch: null,
    run: null,
    missing: false,
    notice: null,
    refreshed: true,
    replaced: false,
    revision,
    ...extra,
  };
}

export const branchKey = (branch: string): BranchKey => ({ kind: "branch", common: "/work/app/.git", branch });

/** A Run's timeline entry: `id` of `status`, `running` while it runs, requested with `comment`. */
export function runChip(id: string, status: "GREEN" | "RED" | "RUNNING", running = false, comment: string | null = null): Chip {
  const glyph = { GREEN: "✓", RED: "✗", RUNNING: "◐" }[status];
  return { kind: "run", id, mark: { status, glyph }, status: running ? "Running" : status, age: running ? "5m" : "5m ago", running, comment };
}

/** Branch `name` of the project `app` with its timeline `chips`, newest first. */
export function branch(name: string, chips: Chip[], older = false, starred = false): BranchView {
  return { key: branchKey(name), project: "app", name, workspace: `/work/${name}`, chips, older, starred };
}

/** A waiting Human review `id` of eval `release/signoff` on `name`. */
export function waitingRow(id: string, name = "main"): WaitingRow {
  return {
    id,
    eval: "release/signoff",
    title: "A person approves the release",
    mark: { status: "WAITING_HUMAN", glyph: "?" },
    project: "app",
    branch: name,
    age: "3m",
    claim: "-",
    mine: false,
  };
}
