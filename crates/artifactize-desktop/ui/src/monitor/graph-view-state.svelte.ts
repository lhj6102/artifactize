import { createContext } from "svelte";

export class GraphViewState {
  canvas = $state<HTMLElement>();
  list = $state<HTMLElement>();
  picked = $state(0);
  /** The Artifact node that should have keyboard focus while the graph has the keyboard. */
  held = $state.raw<string | null>(null);
  /** The number of the latest handoff of focus to a node; a newer one voids older ones. */
  handoff = $state(0);
  /** A frame loop restores the held node's focus. */
  watching = $state(false);
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
