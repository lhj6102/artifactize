import { createContext } from "svelte";
import type { Finding } from "../ipc/client";

export class GraphFindState {
  /** The text typed. */
  text = $state("");
  /** What the library found for `found.text`; the newest answer only. */
  found = $state.raw<{ text: string; finding: Finding } | null>(null);
  /** Why the last find failed. */
  error = $state<string | null>(null);
  /** The chosen entry of the list, or -1 before ↑ or ↓. */
  active = $state(-1);
  /** Numbers the finds asked: only the newest answer shows. */
  asked = 0;
  /** What had focus before the field: Esc gives it back. */
  before: HTMLElement | null = null;
}

export const [getGraphFindState, setGraphFindState] = createContext<GraphFindState>();
