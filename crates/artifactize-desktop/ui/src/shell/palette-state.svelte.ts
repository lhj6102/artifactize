// What the command palette holds of its own: its search field, to put focus in as it opens.
import { createContext } from "svelte";

export class PaletteState {
  input = $state<HTMLInputElement | null>(null);
}

export const [getPaletteState, setPaletteState] = createContext<PaletteState>();
