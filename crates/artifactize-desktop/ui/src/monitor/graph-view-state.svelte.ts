import { createContext } from "svelte";

export class GraphViewState {
  canvas = $state<HTMLElement>();
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
