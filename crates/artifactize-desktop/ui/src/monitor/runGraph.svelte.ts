import { createContext } from "svelte";
import type { RunGraph } from "../ipc/client";
import type { Point } from "./graph";

export class RunGraphState {
  shown = $state.raw<{ graph: RunGraph; positions: Map<string, Point>; shape: string } | null>(null);
  pending = $state.raw<RunGraph | null>(null);
  error = $state<string | null>(null);
  forced = $state(false);
}

export const [getRunGraphState, setRunGraphState] = createContext<RunGraphState>();
