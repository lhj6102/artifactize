// What the Monitor screen holds of its own: which pane has focus, the open Detail, the notice,
// the Artifact whose evals pop up, the node under the pointer and the graph's focus hand-off.
// One owner, shared with createContext; the snapshot it shows is the Browse of the window.
import { createContext } from "svelte";
import { Tree } from "./tree.svelte";

export type Focus = "scope" | "runs" | "graph" | "detail";

export class MonitorState {
  focus = $state<Focus>("runs");
  /** The node whose Detail floats over the graph, in the Run it belongs to. */
  opened = $state.raw<{ run: string; node: string } | null>(null);
  notice = $state<string | null>(null);
  /** The Artifact whose evals pop up beside it. */
  popup = $state<string | null>(null);
  /** The Run the popup belongs to; another Run's graph starts without one. */
  popupRun = $state<string | null>(null);
  /** The Artifact (`[artifact]`) or popup eval (`[artifact, eval]`) under the pointer. */
  hovered = $state.raw<string[] | null>(null);
  /** Gives the mounted graph keyboard focus and says whether it could; null while no graph is. */
  graphFocus = $state.raw<(() => boolean) | null>(null);
  /** Focus was asked for while no graph could take it: the next mounted graph takes it. */
  graphPending = $state(false);
  /** The Runs pane took its first focus: only the first accepted snapshot moves it. */
  started = $state(false);
  /** The Run's selection and blocker jumps. */
  readonly tree = new Tree();
  /** The panes' elements, to move focus into. */
  readonly panes: Partial<Record<Focus, HTMLElement>> = $state({});
}

export const [getMonitorState, setMonitorState] = createContext<MonitorState>();
