// What the Monitor screen holds of its own: which pane has focus, the open Detail and its tab,
// and the notice. One owner, shared with createContext; the snapshot it shows is the Browse of
// the window.
import { createContext } from "svelte";

export type Focus = "scope" | "runs" | "graph" | "detail";

/** An open Detail: an Artifact (or the Run itself) of a Run, and its shown tab, an eval of that
 * Artifact or `null` for the Artifact's own. */
export type Opened = { run: string; node: string; tab: string | null };

export class MonitorState {
  focus = $state<Focus>("runs");
  /** The Detail floating over the graph. */
  opened = $state.raw<Opened | null>(null);
  notice = $state<string | null>(null);
  /** The notice shows in full rather than cut to one line. */
  noticeOpen = $state(false);
  /** The Runs pane took its first focus: only the first accepted snapshot moves it. */
  started = $state(false);
  /** The panes' elements, to move focus into. */
  readonly panes: Partial<Record<Focus, HTMLElement>> = $state({});
}

export const [getMonitorState, setMonitorState] = createContext<MonitorState>();
