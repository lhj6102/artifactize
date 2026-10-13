import { createContext } from "svelte";
import type { Completion } from "../ipc/client";

export class SearchFieldState {
  completion = $state.raw<Completion | null>(null);
  active = $state(-1);
  /** Numbers the completion requests: only the newest answer is used. */
  asked = $state(0);
}

export const [getSearchFieldState, setSearchFieldState] = createContext<SearchFieldState>();
