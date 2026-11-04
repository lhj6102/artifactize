import { createContext } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import type { Shape, ShapeKey, Source } from "../ipc/client";
import type { Point } from "./graph";
import { layout } from "./layout";
import { keyOf } from "./monitor-state.svelte";
import { folded } from "./parts";

/** How many graphs' layouts are kept. */
const KEPT = 8;

/** The graph shown: its source, the shape the backend named, its lit Artifacts and edges, the
 * wholes minimized on it, and where its layout put them. A new snapshot of the same shape only
 * relights it. */
export type Shown = { source: Source; shape: ShapeKey; graph: Shape; minimized: readonly string[]; positions: Map<string, Point> };

export class SourceGraphState {
  shown = $state.raw<Shown | null>(null);
  error = $state<string | null>(null);
  /** Each read of the graph; only the newest one may show what it read. */
  reads = 0;
  /** Layouts by source, shape and minimized wholes, finished or in flight: a read of a shape
   * already laid out, or being laid out, waits for that layout, and returning to a Run or to
   * wholes as they were minimized reuses its own. */
  readonly #layouts = new SvelteMap<string, Promise<Map<string, Point>>>();

  positions(source: Source, shape: ShapeKey, graph: Shape, minimized: readonly string[]): Promise<Map<string, Point>> {
    const id = `${keyOf(source)}\n${shape}\n${folded(graph, minimized).join("\n")}`;
    let laid = this.#layouts.get(id);
    if (!laid) {
      laid = layout(graph, minimized);
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
