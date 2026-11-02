// The graph's camera over its whole canvas. The panels floating over the canvas, the terminal
// on the left and the Detail on the right, take no part: a graph's first view and Fit View show
// the whole graph in the canvas. Only bringing a found Artifact into view minds them, centring it
// in the graph between them.
// Pure geometry over Svelte Flow's public viewport calculation.
import { getViewportForBounds, type Rect, type Viewport, type XYPosition } from "@xyflow/svelte";

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

/** A box on the screen, as getBoundingClientRect gives it. */
export type Box = { left: number; top: number; width: number; height: number };

/** The viewport that puts `point` (in the graph's coordinates) in the middle of the canvas
 * `room` between the panels `covers` floating over it, if any: a panel over the canvas's left
 * half covers it from the left, one over its right half from the right. The zoom stays unless
 * names are too small to read at it, when it rises to `readable`, never beyond `maxZoom`. */
export function centred(point: XYPosition, room: Box, covers: (Box | null)[], zoom: number, readable: number, maxZoom: number): Viewport {
  const scale = Math.min(Math.max(zoom, readable), maxZoom);
  const [right, middle] = [room.left + room.width, room.left + room.width / 2];
  let [from, to] = [room.left, right];
  for (const cover of covers) {
    if (cover === null || cover.left >= right || cover.left + cover.width <= room.left) continue;
    if (cover.left + cover.width / 2 < middle) from = Math.max(from, cover.left + cover.width);
    else to = Math.min(to, cover.left);
  }
  const centre = to > from ? (from + to) / 2 : middle;
  return { x: centre - room.left - point.x * scale, y: room.height / 2 - point.y * scale, zoom: scale };
}
