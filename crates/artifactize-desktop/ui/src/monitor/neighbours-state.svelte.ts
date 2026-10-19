import { createContext } from "svelte";
import type { RunGraph } from "../ipc/client";

/** The Run graph an Artifact's neighbours are drawn from: the library's snapshot, replaced whole
 * on every read. */
export class NeighboursState {
  graph = $state.raw<RunGraph | null>(null);
}

export const [getNeighboursState, setNeighboursState] = createContext<NeighboursState>();
