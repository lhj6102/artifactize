// The palette's search field for focus, and its bound public command selection.
import { createContext } from "svelte";

export class PaletteState {
  input = $state<HTMLInputElement | null>(null);
  selected = $state("");
}

export const [getPaletteState, setPaletteState] = createContext<PaletteState>();
