// The columns right of the Git layer: the Pane and the stage, split by a divider whose sizes
// are shares of their width. The stage never goes narrower than STAGE pixels, which both
// floating panels and some graph need; a layout that would leave it less, kept from before or
// dragged, gives the Pane only what is left.
import { STAGE } from "./detail-width";

/** The Git layer's width in pixels. */
export const GIT = 232;
/** The Pane's narrowest share, in percent, as the divider keeps it. */
const PANE_MIN = 12;
/** The divider's own pixels, besides the panes. */
const DIVIDER = 1;

/** `layout` (the Pane's and the stage's shares) fitted to columns `width` pixels wide, or `null`
 * when it already fits or the columns have no width yet. */
export function fitLayout(layout: number[], width: number): number[] | null {
  if (width <= 0 || layout.length !== 2) return null;
  const stageMin = Math.min(100 - PANE_MIN, ((STAGE + DIVIDER) / width) * 100);
  return layout[1] >= stageMin - 0.01 ? null : [100 - stageMin, stageMin];
}
