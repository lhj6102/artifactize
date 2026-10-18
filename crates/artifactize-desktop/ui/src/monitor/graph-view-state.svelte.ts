import { createContext } from "svelte";

export class GraphViewState {
  canvas = $state<HTMLElement>();
  lastSize = $state<{ width: number; height: number } | null>(null);
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
