import { createContext } from "svelte";
import type { TranscriptView } from "../ipc/client";

export class TranscriptState {
  unavailable = $state<string | null>(null);
  view = $state.raw<TranscriptView | null>(null);
  pane = $state<HTMLElement>();
  probe = $state<HTMLElement>();
}

export const [getTranscriptState, setTranscriptState] = createContext<TranscriptState>();
