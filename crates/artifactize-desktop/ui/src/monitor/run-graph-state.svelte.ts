import { createContext } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import type { RunGraph } from "../ipc/client";
import type { Point } from "./graph";
import { shape } from "./graph";
import { layout } from "./layout";

/** How many Runs' layouts are kept. */
const KEPT = 8;

export class RunGraphState {
  shown = $state.raw<{ graph: RunGraph; positions: Map<string, Point> } | null>(null);
  /** The shape of the graph shown: only a change of Artifacts or edges lays it out again. */
  readonly shownShape = $derived(this.shown ? shape(this.shown.graph) : null);
  pending = $state.raw<RunGraph | null>(null);
  error = $state<string | null>(null);
  /** Lay out a graph above the limit anyway, for this Run. */
  forced = $state(false);
  /** Each read of the graph; only the newest one may show what it read. */
  reads = 0;
  /** The Run last read: another Run starts without the forced layout. */
  last: string | null = null;
  /** Layouts by Run and shape, finished or in flight, so returning to a Run reuses its own. */
  readonly #layouts = new SvelteMap<string, Promise<Map<string, Point>>>();

  positions(graph: RunGraph, key: string): Promise<Map<string, Point>> {
    const id = `${graph.run}\n${key}`;
    let laid = this.#layouts.get(id);
    if (!laid) {
      laid = layout(graph);
      laid.catch(() => this.#layouts.delete(id));
      this.#layouts.set(id, laid);
      // Forget the oldest beyond a few Runs.
      for (const old of [...this.#layouts.keys()].slice(0, Math.max(this.#layouts.size - KEPT, 0))) {
        this.#layouts.delete(old);
      }
    }
    return laid;
  }
}

export const [getRunGraphState, setRunGraphState] = createContext<RunGraphState>();
