// The Detail's width, set by dragging its left edge and kept across launches like the split
// ratio. It is docked at the graph area's right edge and stays within the area: never narrower
// than a readable column, and never so wide that the graph on its left is gone.

/** The narrowest Detail, when the area has room for it. */
export const MIN = 320;
/** What of the graph stays visible left of the widest Detail. */
export const KEEP = 240;

export const PANE_MIN = 12;

/** The graph and its Detail need their two minimum widths when the window has room. */
export function mainSplitMinimum(windowWidth: number): number {
  return Math.min(100 - PANE_MIN, Math.max(30, ((MIN + KEEP + 1) / windowWidth) * 100));
}

const KEY = "monitor-detail-width";

/** `want` pixels, fitted to a graph area `room` pixels wide. */
export function clampWidth(want: number, room: number): number {
  const widest = Math.max(0, room - KEEP);
  const narrowest = Math.min(MIN, widest);
  return Math.round(Math.max(narrowest, Math.min(want, widest)));
}

/** The width kept from an earlier drag, if any. Storage may be unavailable; then there is none. */
export function loadWidth(): number | null {
  try {
    const kept = Number(localStorage.getItem(KEY));
    return Number.isFinite(kept) && kept > 0 ? kept : null;
  } catch {
    return null;
  }
}

/** Keep the dragged width for the next launch; without storage it lasts this window only. */
export function saveWidth(width: number): void {
  try {
    localStorage.setItem(KEY, String(width));
  } catch {
    // Nothing to keep it in.
  }
}
