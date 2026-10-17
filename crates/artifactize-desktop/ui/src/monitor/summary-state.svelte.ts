import { createContext } from "svelte";
import type { DetailView } from "../ipc/client";

/** The Detail a summary was read from, or why it could not be read. */
export class SummaryState {
  detail = $state.raw<DetailView | null>(null);
  error = $state<string | null>(null);
}

export const [getSummaryState, setSummaryState] = createContext<SummaryState>();
