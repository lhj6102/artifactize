import { createContext } from "svelte";
import type { Area } from "./camera";

export class GraphViewState {
  /** The canvas element, while it is mounted. */
  canvas = $state.raw<HTMLElement | null>(null);
  /** The canvas as last measured with a size. */
  area = $state.raw<Area | null>(null);
  /** Svelte Flow measured the first draw. */
  initialized = $state(false);
  /** The first view showed the whole graph; from then on only the person moves the camera. */
  fitted = $state(false);
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
