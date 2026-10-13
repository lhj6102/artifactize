// What the Monitor screen holds of its own: which pane has focus, the Run whose Detail is open,
// the notice in the breadcrumb, the Run view and the zoomed Artifact. One owner, shared with
// createContext; the snapshot it shows is the Browse of the window.
import { createContext } from "svelte";
import { Tree } from "./tree.svelte";

export type Focus = "scope" | "runs" | "tree" | "detail";

export class MonitorState {
  focus = $state<Focus>("runs");
  /** The Run whose Detail is open. The Detail shows the tree's selection, whatever it is. */
  opened = $state<string | null>(null);
  notice = $state<string | null>(null);
  /** The Run as a tree, or as a graph of Artifacts. */
  runView = $state<"tree" | "graph">("tree");
  /** The Artifact the graph is zoomed into. */
  zoomed = $state<string | null>(null);
  /** The Run the zoom belongs to; another Run's graph starts zoomed out. */
  zoomRun = $state<string | null>(null);
  /** The Runs pane took its first focus: only the first accepted snapshot moves it. */
  started = $state(false);
  /** The Run tree's folds and selection. */
  readonly tree = new Tree();
  /** The panes' elements, to move focus into. */
  readonly panes: Partial<Record<Focus, HTMLElement>> = $state({});
}

export const [getMonitorState, setMonitorState] = createContext<MonitorState>();
