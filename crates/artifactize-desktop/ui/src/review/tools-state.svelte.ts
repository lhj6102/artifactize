import { createContext } from "svelte";

/** The reviewer's position in this Tool log, independent of newly arriving lines. */
export class ToolsState {
  following = $state(true);
}

export const [getToolsState, setToolsState] = createContext<ToolsState>();
