// The widths of the panels floating over the graph, the terminal on the left and the Detail on
// the right, each set by dragging its inner edge and kept across launches like the split ratio.
// They stay within the graph's area: never narrower than a readable column, and never so wide
// that the graph between them is gone. The terminal keeps a Detail's narrowest beside it; the
// Detail takes what the terminal leaves. The stage's narrowest fits both at their narrowest with
// some graph between them.

/** A panel's gap from the graph area's edges. */
export const GAP = 12;
/** The narrowest panel, when the area has room for it. */
export const MIN = 320;
/** What of the graph stays visible between the panels. */
export const KEEP = 96;
/** The narrowest stage: both panels at their narrowest, with the graph between them. */
export const STAGE = GAP + MIN + KEEP + MIN + GAP;
/** The terminal's width before any drag. */
export const TERMINAL = 520;

export const DETAIL_KEY = "monitor-detail-width";
export const TERMINAL_KEY = "monitor-terminal-width";

/** `want` pixels, fitted to a graph area `room` pixels wide beside `beside` pixels another
 * panel takes, its gap included. */
export function clampWidth(want: number, room: number, beside = 0): number {
  const widest = room - GAP - KEEP - beside;
  const narrowest = Math.min(MIN, room - 2 * GAP - beside);
  return Math.round(Math.max(narrowest, Math.min(want, widest)));
}

/** The Detail's width before any drag, in a graph area `room` pixels wide. */
export const detailDefault = (room: number) => Math.min(room * 0.46, 680);

/** Both panels' widths in a graph area `room` pixels wide, from what each wants; a panel not
 * shown is `null`. */
export function fitPanels(room: number, terminal: number | null, detail: number | null): { terminal: number | null; detail: number | null } {
  const left = terminal === null ? null : clampWidth(terminal, room, GAP + MIN);
  return { terminal: left, detail: detail === null ? null : clampWidth(detail, room, left === null ? 0 : GAP + left) };
}

/** The width kept under `key` from an earlier drag, if any. Storage may be unavailable; then
 * there is none. */
export function loadWidth(key: string): number | null {
  try {
    const kept = Number(localStorage.getItem(key));
    return Number.isFinite(kept) && kept > 0 ? kept : null;
  } catch {
    return null;
  }
}

/** Keep the dragged width for the next launch; without storage it lasts this window only. */
export function saveWidth(key: string, width: number): void {
  try {
    localStorage.setItem(key, String(width));
  } catch {
    // Nothing to keep it in.
  }
}
