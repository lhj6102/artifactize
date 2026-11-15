import { createContext } from "svelte";

export class HumanDetailState {
  now = $state(Date.now());
  root = $state<HTMLElement>();
  instruction = $state<HTMLElement>();
  /** The review's width in pixels, once measured. */
  width = $state(0);
}

export const [getHumanDetailState, setHumanDetailState] = createContext<HumanDetailState>();
