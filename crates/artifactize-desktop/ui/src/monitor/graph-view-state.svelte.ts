import { createContext } from "svelte";
import type { Area } from "./camera";

export class GraphViewState {
  /** The canvas element, while it is mounted. */
  canvas = $state.raw<HTMLElement | null>(null);
  /** The canvas as last measured with a size. */
  area = $state.raw<Area | null>(null);
  /** How wide the docked Detail covers the canvas's right side, in screen pixels; 0 without one. */
  covered = $state(0);
  /** Svelte Flow measured the first draw. */
  initialized = $state(false);
  /** The first view showed the whole graph; from then on only the person moves the camera. */
  fitted = $state(false);
  /** The canvas grid's cell at zoom 1, in screen pixels, as the --grid token sets it. */
  cell = $state(24);
  /** An Artifact's name's font at zoom 1, in screen pixels, as the --text-body token sets it. */
  font = $state(13);
  /** The drawn copy focus is on, by key. */
  focused = $state<string | null>(null);
  /** A whole's face was just dragged to pan: the click that ends it opens nothing. */
  panned = false;
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();

/** The graph view a node is drawn in; none for a node drawn elsewhere, such as the Detail's
 * Neighbours. */
export function graphViewStateOf(): GraphViewState | null {
  try {
    return getGraphViewState();
  } catch {
    return null;
  }
}
