// The Detail's width, set by dragging its left edge and kept across launches like the split
// ratio. It stays within the graph's area: never narrower than a readable column, and never
// so wide that the graph behind it is gone.

/** The Detail's gap from the graph area's edges. */
export const GAP = 12;
/** The narrowest Detail, when the area has room for it. */
export const MIN = 320;
/** What of the graph stays visible left of the widest Detail. */
export const KEEP = 96;

const KEY = "monitor-detail-width";

/** `want` pixels, fitted to a graph area `room` pixels wide. */
export function clampWidth(want: number, room: number): number {
  const widest = room - GAP - KEEP;
  const narrowest = Math.min(MIN, room - 2 * GAP);
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
