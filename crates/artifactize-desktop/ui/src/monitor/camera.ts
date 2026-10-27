// The graph's camera over its whole canvas. A panel floating over the canvas, such as the
// Detail, takes no part: a graph's first view and Fit View show the whole graph in the canvas.
// Only bringing a found Artifact into view minds the Detail, centring it in the graph left of it.
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
 * `room` left of the panel `cover` floating over it, if any: the zoom stays unless names are
 * too small to read at it, when it rises to `readable`, never beyond `maxZoom`. */
export function centred(point: XYPosition, room: Box, cover: Box | null, zoom: number, readable: number, maxZoom: number): Viewport {
  const scale = Math.min(Math.max(zoom, readable), maxZoom);
  const covered = cover !== null && cover.left > room.left && cover.left < room.left + room.width;
  const width = covered ? cover.left - room.left : room.width;
  return { x: width / 2 - point.x * scale, y: room.height / 2 - point.y * scale, zoom: scale };
}
