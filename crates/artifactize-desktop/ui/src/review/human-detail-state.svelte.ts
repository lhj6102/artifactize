import { createContext } from "svelte";

export class HumanDetailState {
  logOpen = $state(false);
  root = $state<HTMLElement>();
  instruction = $state<HTMLElement>();
}

export const [getHumanDetailState, setHumanDetailState] = createContext<HumanDetailState>();
