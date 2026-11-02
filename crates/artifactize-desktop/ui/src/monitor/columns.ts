// The columns right of the Git layer: the Pane and the stage, split by a divider whose sizes
// are shares of their width. The stage never goes narrower than STAGE pixels, which both
// floating panels and some graph need; the Pane's widest leaves it that, a layout kept from
// before included.
import { STAGE } from "./detail-width";

/** The Git layer's width in pixels. */
export const GIT = 232;
/** The Pane's narrowest share, in percent. */
export const PANE_MIN = 12;
/** The divider's own pixels, besides the panes. */
const DIVIDER = 1;

/** The Pane's widest and the stage's narrowest share, in percent, of columns `width` pixels
 * wide; before they are measured, the stage keeps 30%. */
export function limits(width: number): { paneMax: number; stageMin: number } {
  const stageMin = width > 0 ? Math.min(100 - PANE_MIN, ((STAGE + DIVIDER) / width) * 100) : 30;
  return { paneMax: 100 - stageMin, stageMin };
}

/** `layout` (the Pane's and the stage's shares) fitted to columns `width` pixels wide, or `null`
 * when it already fits. */
export function fitLayout(layout: number[], width: number): number[] | null {
  const { paneMax } = limits(width);
  if (layout.length !== 2 || layout[0] <= paneMax + 0.01) return null;
  return [paneMax, 100 - paneMax];
}
