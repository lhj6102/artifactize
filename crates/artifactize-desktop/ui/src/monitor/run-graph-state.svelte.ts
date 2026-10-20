import { createContext } from "svelte";
import type { Shape, ShapeKey } from "../ipc/client";
import type { Point } from "./graph";

/** The Run graph shown: the Run, the shape the backend named, its lit Artifacts and edges, and
 * where its layout put them. A new snapshot of the same shape only relights it. */
export type Shown = { run: string; shape: ShapeKey; graph: Shape; positions: Map<string, Point> };

export class RunGraphState {
  shown = $state.raw<Shown | null>(null);
  error = $state<string | null>(null);
  /** Each read of the graph; only the newest one may show what it read. */
  reads = 0;
}

export const [getRunGraphState, setRunGraphState] = createContext<RunGraphState>();
