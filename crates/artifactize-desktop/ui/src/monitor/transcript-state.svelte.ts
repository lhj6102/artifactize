import { createContext } from "svelte";
import type { Movement, TranscriptView } from "../ipc/client";

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
  /** A window is being asked for; requests made meanwhile merge into one follow-up. */
  asking = $state(false);
  again = $state.raw<{ movement: Movement | null; toggle: number | null; changed: boolean } | null>(null);
  /** The session is being resolved. */
  resolving = $state(false);
  /** The pane is gone: nothing more is asked or resolved. */
  closed = $state(false);
}

export const [getTranscriptState, setTranscriptState] = createContext<TranscriptState>();
