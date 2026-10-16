// The palette's search field for focus, its typed search, and its bound public command selection.
import { createContext } from "svelte";

export class PaletteState {
  input = $state<HTMLInputElement | null>(null);
  /** Cleared when the palette closes: a palette opened again before the closing one has left
   * the page keeps its content, so every opening starts from an empty search. */
  search = $state("");
  selected = $state("");
}

export const [getPaletteState, setPaletteState] = createContext<PaletteState>();
