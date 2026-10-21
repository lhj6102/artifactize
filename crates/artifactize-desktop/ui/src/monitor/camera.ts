// The graph's camera over its whole canvas. A panel floating over the canvas, such as the
// Detail, takes no part: a graph's first view and Fit View show the whole graph in the canvas.
// Pure geometry over Svelte Flow's public viewport calculation.
import { getViewportForBounds, type Rect, type Viewport } from "@xyflow/svelte";

/** The canvas's size in pixels. */
export type Area = { width: number; height: number };

/** How a view fits its whole graph: the largest zoom, and the padding around the graph as a
 * fraction of the canvas, as Svelte Flow reads a numeric padding. */
export type Fit = { minZoom: number; maxZoom: number; padding: number };

/** The canvas of `width` × `height` pixels; null while it has no size. */
export function area(width: number, height: number): Area | null {
  return width > 0 && height > 0 ? { width, height } : null;
}

/** The viewport that shows all of `bounds` within the canvas `at`. */
export function whole(bounds: Rect, at: Area, fit: Fit): Viewport {
  return getViewportForBounds(bounds, at.width, at.height, fit.minZoom, fit.maxZoom, fit.padding);
}
