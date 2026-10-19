// What the Monitor screen holds of its own: which pane has focus, the open Detail, its tab and
// its width, and whether the notice shows in full. One owner, shared with createContext; the
// snapshot it shows is the Browse of the window.
import { createContext } from "svelte";
import { loadWidth } from "./detail-width";

export type Focus = "scope" | "runs" | "graph" | "detail";

/** An open Detail: an Artifact (or the Run itself) of a Run, its shown tab, an eval of that
 * Artifact or `null` for the Artifact's own, and the Human request whose review that tab shows,
 * once its Detail arrived. */
export type Opened = { run: string; node: string; tab: string | null; request?: string };

export class MonitorState {
  focus = $state<Focus>("runs");
  /** The Detail floating over the graph. */
  opened = $state.raw<Opened | null>(null);
  /** The Detail's dragged width in pixels, or `null` for its default. */
  detailWidth = $state<number | null>(loadWidth());
  /** The notice shows in full rather than cut to one line. */
  noticeOpen = $state(false);
  /** The Runs pane took its first focus: only the first accepted snapshot moves it. */
  started = $state(false);
  /** The panes' elements, to move focus into. */
  readonly panes: Partial<Record<Focus, HTMLElement>> = $state({});
}

export const [getMonitorState, setMonitorState] = createContext<MonitorState>();
