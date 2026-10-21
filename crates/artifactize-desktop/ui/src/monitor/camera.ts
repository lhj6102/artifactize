// The graph's camera beside a panel docked over the canvas's right side, such as the Detail.
// The part of the canvas left of the panel is what a person can see and click: a graph's first
// view and Fit View show the whole graph in that part. Pure geometry over Svelte Flow's public
// viewport calculation.
import { getViewportForBounds, type Rect, type Viewport } from "@xyflow/svelte";

/** The canvas: its size, and how much of its right side a docked panel covers. */
export type Area = { width: number; height: number; covered: number };

/** How a view fits its whole graph: the largest zoom, and the padding around the graph as a
 * fraction of the canvas, as Svelte Flow reads a numeric padding. */
export type Fit = { minZoom: number; maxZoom: number; padding: number };

type Box = { left: number; top: number; right: number; bottom: number; width: number; height: number };

/** The canvas of `width` × `height` pixels, placed at `canvas`, under the panel placed at
 * `panel` (null when none is open); null while the canvas has no size. A panel beside the
 * canvas's vertical extent covers nothing. */
export function area(width: number, height: number, canvas: Box, panel: Box | null): Area | null {
  if (width <= 0 || height <= 0) return null;
  const over = panel !== null && panel.width > 0 && panel.top < canvas.bottom && panel.bottom > canvas.top;
  const covered = over ? Math.min(Math.max(canvas.right - panel.left, 0), width) : 0;
  return { width, height, covered };
}

/** The padding that keeps a fit left of the panel: the view's own on every side, and the
 * covered width more on the right. Fit View takes the same, so its button fits the same way. */
export function padding(at: Area | null, fit: Fit): Fit["padding"] | { x: number; y: number; right: `${number}px` } {
  if (!at || at.covered === 0) return fit.padding;
  // Svelte Flow's own reading of a numeric padding, for one side of the canvas's width.
  const side = Math.floor((at.width - at.width / (1 + fit.padding)) * 0.5);
  return { x: fit.padding, y: fit.padding, right: `${at.covered + side}px` };
}

/** The viewport that shows all of `bounds` within the part of `at` the panel leaves. */
export function whole(bounds: Rect, at: Area, fit: Fit): Viewport {
  return getViewportForBounds(bounds, at.width, at.height, fit.minZoom, fit.maxZoom, padding(at, fit));
}
