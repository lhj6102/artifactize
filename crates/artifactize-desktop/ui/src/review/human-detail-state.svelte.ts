import { createContext } from "svelte";

export class HumanDetailState {
  now = $state(Date.now());
  root = $state<HTMLElement>();
  instruction = $state<HTMLElement>();
}

export const [getHumanDetailState, setHumanDetailState] = createContext<HumanDetailState>();
