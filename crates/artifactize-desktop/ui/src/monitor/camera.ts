// The graph's camera over its protected view: the canvas less the status line's room at its top
// (D-MAIN-2) and less the band a floating chat covers at its bottom (D-CUR-10). A panel docked
// over the canvas, such as the Detail, takes no part in fitting: a graph's first view and Fit
// View show the whole graph in the protected view. Bringing a found, created or keyboard-reached
// Artifact into view also minds the Detail, placing it in the protected view left of it.
// Pure geometry over Svelte Flow's public viewport calculation.
import { getViewportForBounds, type Rect, type Viewport, type XYPosition } from "@xyflow/svelte";

/** The canvas's size in pixels. */
export type Area = { width: number; height: number };

/** How a view fits its whole graph: the largest zoom, and the padding around the graph as a
 * fraction of the canvas, as Svelte Flow reads a numeric padding. */
export type Fit = { minZoom: number; maxZoom: number; padding: number };

/** The pixels kept free of the graph at the canvas's top and bottom. */
export type Insets = { top: number; bottom: number };

export const NO_INSETS: Insets = { top: 0, bottom: 0 };

/** The canvas of `width` × `height` pixels; null while it has no size. */
export function area(width: number, height: number): Area | null {
  return width > 0 && height > 0 ? { width, height } : null;
}

/** The protected view's height in the canvas `at`, never below one pixel. */
function room(height: number, insets: Insets): number {
  return Math.max(1, height - insets.top - insets.bottom);
}

/** The viewport that shows all of `bounds` within the protected view of the canvas `at`. */
export function whole(bounds: Rect, at: Area, fit: Fit, insets: Insets = NO_INSETS): Viewport {
  const shown = getViewportForBounds(bounds, at.width, room(at.height, insets), fit.minZoom, fit.maxZoom, fit.padding);
  return { ...shown, y: shown.y + insets.top };
}

/** The viewport zoomed to `zoom` around the protected view's centre in the canvas `at`: the
 * point of the graph there stays there. */
export function zoomedAround(now: Viewport, zoom: number, at: Area, insets: Insets = NO_INSETS): Viewport {
  const centre = { x: at.width / 2, y: insets.top + room(at.height, insets) / 2 };
  const ratio = zoom / now.zoom;
  return { x: centre.x - (centre.x - now.x) * ratio, y: centre.y - (centre.y - now.y) * ratio, zoom };
}

/** A box on the screen, as getBoundingClientRect gives it. */
export type Box = { left: number; top: number; width: number; height: number };

/** How wide the canvas `room` shows left of the panel `cover` docked over it, if any. */
function uncovered(room: Box, cover: Box | null): number {
  const covered = cover !== null && cover.left > room.left && cover.left < room.left + room.width;
  return covered ? cover.left - room.left : room.width;
}

/** The viewport that puts `point` (in the graph's coordinates) in the middle of the protected
 * view of the canvas `canvas` left of the panel `cover` docked over it, if any: the zoom stays
 * unless names are too small to read at it, when it rises to `readable`, never beyond
 * `maxZoom`. */
export function centred(
  point: XYPosition,
  canvas: Box,
  cover: Box | null,
  zoom: number,
  readable: number,
  maxZoom: number,
  insets: Insets = NO_INSETS,
): Viewport {
  const scale = Math.min(Math.max(zoom, readable), maxZoom);
  const width = uncovered(canvas, cover);
  const middle = insets.top + room(canvas.height, insets) / 2;
  return { x: width / 2 - point.x * scale, y: middle - point.y * scale, zoom: scale };
}

/** The viewport moved as little as needed to bring `box` (in the graph's coordinates) into the
 * protected view of `canvas` left of `cover`, the zoom kept; `now` itself when it is in view. A
 * box larger than the view keeps its top left corner in view. */
export function revealed(box: Rect, now: Viewport, canvas: Box, cover: Box | null, insets: Insets = NO_INSETS): Viewport {
  const width = uncovered(canvas, cover);
  const [top, bottom] = [insets.top, insets.top + room(canvas.height, insets)];
  const shift = (start: number, end: number, low: number, high: number) =>
    start < low ? low - start : end > high ? Math.max(high - end, low - start) : 0;
  const left = now.x + box.x * now.zoom;
  const upper = now.y + box.y * now.zoom;
  const dx = shift(left, left + box.width * now.zoom, 0, width);
  const dy = shift(upper, upper + box.height * now.zoom, top, bottom);
  return dx === 0 && dy === 0 ? now : { x: now.x + dx, y: now.y + dy, zoom: now.zoom };
}
