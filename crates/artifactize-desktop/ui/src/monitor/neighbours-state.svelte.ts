import { createContext } from "svelte";
import type { Graph } from "../ipc/client";

/** The graph an Artifact's neighbours are drawn from: the library's snapshot or scan, replaced whole
 * on every read. */
export class NeighboursState {
  graph = $state.raw<Graph | null>(null);
}

export const [getNeighboursState, setNeighboursState] = createContext<NeighboursState>();
