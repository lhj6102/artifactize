// What the window's one screen holds of its own: where focus is, the Pane's selected entry,
// what the Main area shows, the open Detail with its tab and width, and whether the notice
// shows in full. One owner, kept in the AppState; the snapshot it shows is the Browse.
import type { BranchKey, DetailView, Source } from "../ipc/client";
import { loadWidth } from "./detail-width";

export type Focus = "pane" | "main" | "detail";

/** The Pane's selected entry: a branch, or a waiting review. */
export type Entry = { kind: "branch"; branch: BranchKey } | { kind: "waiting"; id: string };

/** What the Main area shows. */
export type Shown =
  | { kind: "none" }
  | { kind: "run"; branch: BranchKey; run: string }
  | { kind: "current"; branch: BranchKey }
  | { kind: "review"; id: string }
  | { kind: "request"; detail: DetailView };

/** An open Detail: an Artifact (or the Run itself) of a Run or a Current, its shown tab, an
 * eval of that Artifact or `null` for the Artifact's own, and the Human request whose review
 * that tab shows, once its Detail arrived. */
export type Opened = { source: Source; node: string; tab: string | null; request?: string };

/** One text for a value the backend sends: branches and sources compare by it. */
export const keyOf = (value: BranchKey | Source): string => JSON.stringify(value);

/** The graph source a shown Run or Current reads from. */
export function sourceOf(shown: Shown): Source | null {
  if (shown.kind === "run") return { kind: "run", run: shown.run };
  if (shown.kind === "current") return { kind: "current", branch: shown.branch };
  return null;
}

export class MonitorState {
  focus = $state<Focus>("pane");
  selected = $state.raw<Entry | null>(null);
  shown = $state.raw<Shown>({ kind: "none" });
  /** Settings show in the Main area over what it showed before. */
  settings = $state(false);
  /** The Detail floating over the graph. */
  opened = $state.raw<Opened | null>(null);
  /** The Detail's docked box while it is open: the graph keeps its Artifacts out from under it. */
  dock = $state<HTMLElement | null>(null);
  /** The Detail's dragged width in pixels, or `null` for its default. */
  detailWidth = $state<number | null>(loadWidth());
  /** The notice shows in full rather than cut to one line. */
  noticeOpen = $state(false);
  /** What the launch asked for was opened: only the first snapshot opens it. */
  started = $state(false);
  /** The parts focus moves into. */
  readonly panes: { pane?: HTMLElement; main?: HTMLElement; detail?: HTMLElement; search?: HTMLInputElement } = $state({});
}
