// What Verify holds of its own: where focus is, the Pane's selected entry, what the Main area
// shows, the open Detail with its tab and width, whether the notice shows in full, and the
// wholes minimized on each branch's graphs. One owner, kept in the AppState; the snapshot it
// shows is the Browse.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import type { BranchKey, Chip, Source } from "../ipc/client";
import { GraphWorkspace } from "./graph-workspace.svelte";
export type { Focus } from "./graph-workspace.svelte";

/** The Pane's selected entry: a branch, or one entry of an expanded branch's timeline (a Run by
 * its ID). */
export type Entry = { kind: "branch"; branch: BranchKey } | { kind: "chip"; branch: BranchKey; chip: string };

/** What the Main area shows. */
export type Shown = { kind: "none" } | { kind: "run"; branch: BranchKey; run: string } | { kind: "branch"; branch: BranchKey };

/** An open Detail: an Artifact (or the Run itself) of a Run or a Current, its shown tab, an
 * eval of that Artifact or `null` for the Artifact's own, and the Human request whose review
 * that tab shows, once its Detail arrived. */
export type Opened = { source: Source; node: string; tab: string | null; request?: string };

/** One text for a value the backend sends: branches and sources compare by it. */
export const keyOf = (value: BranchKey | Source): string => JSON.stringify(value);

/** A timeline entry's name among its branch's: a Run's ID. */
export const chipKey = (chip: Chip): string => chip.id;

/** One text for an entry: the selection compares by it. */
export function entryKey(entry: Entry | null): string | null {
  if (entry === null) return null;
  return entry.kind === "branch" ? keyOf(entry.branch) : `${keyOf(entry.branch)} ${entry.chip}`;
}

/** The graph source a shown Run reads from. */
export function sourceOf(shown: Shown): Source | null {
  if (shown.kind === "run") return { kind: "run", run: shown.run };
  return null;
}

export class MonitorState extends GraphWorkspace {
  selected = $state.raw<Entry | null>(null);
  /** The branches expanded to their timelines, by key; any number, for this window. */
  readonly expanded = new SvelteSet<string>();
  shown = $state.raw<Shown>({ kind: "none" });
  /** Settings show in the Main area over what it showed before. */
  settings = $state(false);
  runRead = $state.raw<{ id: string; phase: "loading" | "unreadable" } | null>(null);
  /** The notice shows in full rather than cut to one line. */
  noticeOpen = $state(false);
  /** What the launch asked for was opened: only the first snapshot opens it. */
  started = $state(false);
  /** The wholes minimized on each branch's graphs, its Runs' and its Current, by the branch's
   * key: their tree ids, kept while the app runs. */
  readonly minimized = new SvelteMap<string, readonly string[]>();

  /** The wholes minimized on `branch`'s graphs. */
  minimizedOn(branch: BranchKey): readonly string[] {
    return this.minimized.get(keyOf(branch)) ?? NONE;
  }

  /** Minimize the whole `id` on `branch`'s graphs, or expand it. */
  minimize(branch: BranchKey, id: string, minimized: boolean) {
    // A new list, never changed: a graph laid out for the one before keeps it.
    const others = this.minimizedOn(branch).filter((each) => each !== id);
    this.minimized.set(keyOf(branch), minimized ? [...others, id] : others);
  }
}

/** No whole minimized. */
const NONE: readonly string[] = [];
