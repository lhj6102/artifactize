import { createContext } from "svelte";
import type { Area } from "./camera";
import type { Point } from "./graph";

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
  /** The node being dragged, and whether Esc put it back. */
  dragging = $state.raw<{ key: string; cancelled: boolean } | null>(null);
  /** Where the pointer last went down on the graph: a dropped node moved as far as the pointer
   * did from there, whatever frames its drawing skipped. */
  pressed = $state.raw<Point | null>(null);
  /** Where the node being dragged is now, as its drag last moved it: the nodes are drawn with it
   * there whenever they are drawn again, a drop target's highlight included. Read untracked, so
   * a move alone redraws nothing. */
  lifted = $state.raw<{ key: string; position: Point } | null>(null);
  /** The drawn copy focus is on, by key. */
  focused = $state<string | null>(null);
  /** A whole's face was just dragged to pan: the click that ends it opens nothing. */
  panned = false;
}

export const [getGraphViewState, setGraphViewState] = createContext<GraphViewState>();
