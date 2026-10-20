import { createContext } from "svelte";
import type { Neighbours } from "../ipc/client";

/** An Artifact between its neighbours, as the library's snapshot has them: replaced whole on
 * every read. */
export class NeighboursState {
  graph = $state.raw<Neighbours | null>(null);
}

export const [getNeighboursState, setNeighboursState] = createContext<NeighboursState>();
