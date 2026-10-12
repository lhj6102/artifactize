import { createContext } from "svelte";

export class RunPaneState {
  list = $state<HTMLElement>();
  offscreen = $state({ above: 0, below: 0 });
}

export const [getRunPaneState, setRunPaneState] = createContext<RunPaneState>();
