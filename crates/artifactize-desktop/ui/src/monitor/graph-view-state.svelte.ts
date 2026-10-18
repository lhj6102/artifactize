import { createContext } from "svelte";

export class GraphViewState {
  canvas = $state<HTMLElement>();
  lastSize = $state<{ width: number; height: number } | null>(null);
  /** Svelte Flow measured the first draw and fitted it into view. */
  initialized = $state(false);
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
