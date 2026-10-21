// What the window's one screen holds of its own: where focus is, the Pane's selected entry,
// what the Main area shows, the open Detail with its tab and width, and whether the notice
// shows in full. One owner, kept in the AppState; the snapshot it shows is the Browse.
import { SvelteSet } from "svelte/reactivity";
import type { BranchKey, Chip, DetailView, Source } from "../ipc/client";
import { loadWidth } from "./detail-width";

export type Focus = "pane" | "main" | "detail";

/** The Pane's selected entry: a branch, one entry of an expanded branch's timeline (a Run by
 * its ID, the Current as `current`), or a waiting review. */
export type Entry =
  | { kind: "branch"; branch: BranchKey }
  | { kind: "chip"; branch: BranchKey; chip: string }
  | { kind: "waiting"; id: string };

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

/** A timeline entry's name among its branch's: a Run's ID, or `current`. */
export const chipKey = (chip: Chip): string => (chip.kind === "run" ? chip.id : "current");

/** One text for an entry: the selection compares by it. */
export function entryKey(entry: Entry | null): string | null {
  if (entry === null) return null;
  if (entry.kind === "waiting") return entry.id;
  return entry.kind === "branch" ? keyOf(entry.branch) : `${keyOf(entry.branch)} ${entry.chip}`;
}

/** The graph source a shown Run or Current reads from. */
export function sourceOf(shown: Shown): Source | null {
  if (shown.kind === "run") return { kind: "run", run: shown.run };
  if (shown.kind === "current") return { kind: "current", branch: shown.branch };
  return null;
}

export class MonitorState {
  focus = $state<Focus>("pane");
  selected = $state.raw<Entry | null>(null);
  /** The branches expanded to their timelines, by key; any number, for this window. */
  readonly expanded = new SvelteSet<string>();
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
  /** The read-only request's folds. */
  folds = $state({ technical: false, pending: false, log: false, debug: false });
  /** The ID typed into the Open by ID dialog. */
  wanted = $state("");
  /** The snapshot revision a request shown by ID was last read for; never drawn. */
  followed = $state(0);
  /** The Runs each branch had in progress by its key, to notice one ending; never drawn. */
  running = $state<Record<string, string[]>>({});
  /** What the launch asked for was opened: only the first snapshot opens it. */
  started = $state(false);
  /** The parts focus moves into. */
  readonly panes: { pane?: HTMLElement; main?: HTMLElement; detail?: HTMLElement; search?: HTMLInputElement } = $state({});
}
