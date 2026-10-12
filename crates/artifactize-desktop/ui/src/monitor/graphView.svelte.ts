import { createContext } from "svelte";

export class GraphViewState {
  canvas = $state<HTMLElement>();
  list = $state<HTMLElement>();
  picked = $state(0);
  /** The Artifact node that should have keyboard focus while the graph has the keyboard. */
  held = $state.raw<string | null>(null);
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
