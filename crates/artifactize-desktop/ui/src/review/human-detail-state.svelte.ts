import { createContext } from "svelte";

export class HumanDetailState {
  logOpen = $state(false);
  root = $state<HTMLElement>();
  instruction = $state<HTMLElement>();
  /** The review's width in pixels, once measured. */
  width = $state(0);
}

export const [getHumanDetailState, setHumanDetailState] = createContext<HumanDetailState>();
