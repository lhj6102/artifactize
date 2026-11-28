import { createContext } from "svelte";

export class HumanDetailState {
  root = $state<HTMLElement>();
  /** The conversation's scrolling part. */
  convo = $state<HTMLElement>();
  /** What this surface's issues field holds and has not added: its own until Enter adds it. */
  typed = $state("");
}

export const [getHumanDetailState, setHumanDetailState] = createContext<HumanDetailState>();
