import { createContext } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import type { Graph } from "../ipc/client";
import type { Point } from "./graph";
import { shape } from "./graph";
import { layout } from "./layout";
import { keyOf } from "./monitor-state.svelte";

/** How many graphs' layouts are kept. */
const KEPT = 8;

export class SourceGraphState {
  shown = $state.raw<{ graph: Graph; positions: Map<string, Point> } | null>(null);
  /** The shape of the graph shown: only a change of Artifacts or edges lays it out again. */
  readonly shownShape = $derived(this.shown ? shape(this.shown.graph) : null);
  pending = $state.raw<Graph | null>(null);
  error = $state<string | null>(null);
  /** Lay out a graph above the limit anyway, for this source. */
  forced = $state(false);
  /** Each read of the graph; only the newest one may show what it read. */
  reads = 0;
  /** The source last read: another one starts without the forced layout. */
  last: string | null = null;
  /** Layouts by source and shape, finished or in flight, so returning to a Run reuses its own. */
  readonly #layouts = new SvelteMap<string, Promise<Map<string, Point>>>();

  positions(graph: Graph, key: string): Promise<Map<string, Point>> {
    const id = `${keyOf(graph.source)}\n${key}`;
    let laid = this.#layouts.get(id);
    if (!laid) {
      laid = layout(graph);
      laid.catch(() => this.#layouts.delete(id));
      this.#layouts.set(id, laid);
      // Forget the oldest beyond a few graphs.
      for (const old of [...this.#layouts.keys()].slice(0, Math.max(this.#layouts.size - KEPT, 0))) {
        this.#layouts.delete(old);
      }
    }
    return laid;
  }
}

export const [getSourceGraphState, setSourceGraphState] = createContext<SourceGraphState>();
