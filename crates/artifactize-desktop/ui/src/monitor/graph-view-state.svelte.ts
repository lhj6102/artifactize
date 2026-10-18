import { createContext } from "svelte";

export class GraphViewState {
  canvas = $state<HTMLElement>();
  lastSize = $state<{ width: number; height: number } | null>(null);
  /** Svelte Flow measured the first draw and fitted it into view. */
  initialized = $state(false);
  /** The graph was fitted into a canvas that had a size. */
  fitted = $state(false);
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
