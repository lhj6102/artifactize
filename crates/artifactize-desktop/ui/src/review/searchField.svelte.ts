import { createContext } from "svelte";
import type { Completion } from "../ipc/client";

export class SearchFieldState {
  completion = $state.raw<Completion | null>(null);
  active = $state(-1);
}

export const [getSearchFieldState, setSearchFieldState] = createContext<SearchFieldState>();
