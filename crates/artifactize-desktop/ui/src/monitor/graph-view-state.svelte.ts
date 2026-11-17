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
  /** The canvas grid's cell at zoom 1, in screen pixels, as the --grid token sets it. */
  cell = $state(24);
  /** An Artifact's name's font at zoom 1, in screen pixels, as the --text-body token sets it. */
  font = $state(13);
  /** Grows when what was dragged goes back where it was: the nodes are drawn from the layout
   * again. */
  epoch = $state(0);
  /** The copy a release would drop on, highlighted while the drop would be accepted. */
  target = $state<string | null>(null);
  /** A comment being dragged: whose copy, and where the pointer is in the canvas. */
  comment = $state.raw<{ key: string; x: number; y: number } | null>(null);
  /** A whole's face was just dragged to pan: the click that ends it opens nothing. */
  panned = false;
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
