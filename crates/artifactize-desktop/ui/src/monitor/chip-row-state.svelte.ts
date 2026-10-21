import { createContext } from "svelte";

/** Whether a branch's chip row stays at its newest end: until the person moves it, by a drag
 * or by choosing a chip the row scrolls to. */
export class ChipRowState {
  pinned = $state(true);
}

export const [getChipRowState, setChipRowState] = createContext<ChipRowState>();
