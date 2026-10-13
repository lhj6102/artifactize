// Current project structure has one owner. Refresh explicitly reloads declarations, never
// reads a historical Run or writes the state database.
import { createContext } from "svelte";
import { call, commands, message, type CurrentGraph } from "../ipc/client";
import { layout } from "./layout";
import type { Point } from "./graph";

export const [getCurrentGraphState, setCurrentGraphState] = createContext<CurrentGraphState>();

export class CurrentGraphState {
  shown = $state.raw<{ view: CurrentGraph; positions: Map<string, Point> } | null>(null);
  error = $state<string | null>(null);
  selected = $state<string[]>([]);
  zoomed = $state<string | null>(null);
  reading = $state(false);
  details = $state(false);
  #read = 0;

  async refresh(): Promise<void> {
    const read = ++this.#read;
    this.reading = true;
    try {
      const view = await call(commands.currentGraph());
      const positions = await layout(view.graph);
      if (read !== this.#read) return;
      this.shown = { view, positions };
      this.error = null;
      this.selected = [];
      this.zoomed = null;
    } catch (error) {
      if (read === this.#read) this.error = message(error);
    } finally {
      if (read === this.#read) this.reading = false;
    }
  }
}
