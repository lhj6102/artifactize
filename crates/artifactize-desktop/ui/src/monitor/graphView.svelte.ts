import { createContext } from "svelte";

export class GraphViewState {
  canvas = $state<HTMLElement>();
  list = $state<HTMLElement>();
  picked = $state(0);
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
