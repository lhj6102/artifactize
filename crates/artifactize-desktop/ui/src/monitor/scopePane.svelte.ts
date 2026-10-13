import { createContext } from "svelte";

export class ScopePaneState {
  cursor = $state<number | null>(null);
}

export const [getScopePaneState, setScopePaneState] = createContext<ScopePaneState>();
