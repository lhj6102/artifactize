import { createContext } from "svelte";

export class RunPaneState {
  list = $state<HTMLElement>();
  /** An explicit view switch waiting for its tree or asynchronously loaded graph to mount. */
  focusPending = $state<"tree" | "graph" | null>(null);
  offscreen = $state({ above: 0, below: 0 });
}

export const [getRunPaneState, setRunPaneState] = createContext<RunPaneState>();
