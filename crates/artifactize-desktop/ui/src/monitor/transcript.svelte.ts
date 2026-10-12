import { createContext } from "svelte";
import type { TranscriptView } from "../ipc/client";

export class TranscriptState {
  unavailable = $state<string | null>(null);
  /** The open reader, and the session it was resolved for. */
  handle = $state.raw<number | null>(null);
  session = $state.raw<string | null>(null);
  /** The pane's size in text columns and rows, as last measured. */
  size = $state.raw({ columns: 0, rows: 0 });
  view = $state.raw<TranscriptView | null>(null);
  pane = $state<HTMLElement>();
  probe = $state<HTMLElement>();
}

export const [getTranscriptState, setTranscriptState] = createContext<TranscriptState>();
