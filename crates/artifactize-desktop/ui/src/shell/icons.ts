// The app's own marks, as the design's icon sheet draws them: each on a 12 by 12 box with a
// 1.5 stroke, round caps and joins, solid only where the shape is a solid. An icon takes the
// colour of the text around it. The same shapes paint a large graph's canvas (scene.ts).
import type { Light, Status, Tone } from "../ipc/client";

/** One shape of an icon: an SVG path, filled or stroked, a stroke dashed or thinner. */
export type Shape = { d: string; fill?: boolean; dash?: [number, number]; width?: number };

/** A circle as a path, so a canvas can draw it from the same data. */
const circle = (cx: number, cy: number, r: number) => `M${cx - r} ${cy}a${r} ${r} 0 1 0 ${2 * r} 0a${r} ${r} 0 1 0 ${-2 * r} 0`;

const STAR = "M6 1.5l1.35 2.9 3.15.4-2.3 2.2.6 3.2L6 8.65 3.2 10.2l.6-3.2-2.3-2.2 3.15-.4z";

const DRAWN = {
  green: [{ d: "M2.5 6.5 5 9l4.5-6" }],
  red: [{ d: "M3 3l6 6M9 3l-6 6" }],
  error: [{ d: "M6 2v5M6 9.75v.01" }],
  running: [{ d: circle(6, 6, 4.25) }, { d: "M6 1.75a4.25 4.25 0 0 0 0 8.5z", fill: true }],
  human: [{ d: circle(6, 3.75, 2) }, { d: "M2.25 10.5c.4-2 1.9-3 3.75-3s3.35 1 3.75 3" }],
  queued: [{ d: circle(6, 6, 2), fill: true }],
  waiting: [{ d: circle(2.25, 6, 1.1), fill: true }, { d: circle(6, 6, 1.1), fill: true }, { d: circle(9.75, 6, 1.1), fill: true }],
  blocked: [{ d: circle(6, 6, 4.25) }, { d: "M3 9l6-6" }],
  notrun: [{ d: circle(6, 6, 4.25) }],
  run: [{ d: "M4 2.75v6.5L9.5 6z" }],
  reused: [{ d: "M9.75 6a3.75 3.75 0 1 1-1.2-2.75M9.75 2v2.25H7.5" }],
  checking: [{ d: circle(6, 6, 4.25), dash: [1.4, 2.05] }],
  play: [{ d: "M3.5 2.25v7.5L10 6z", fill: true }],
  star: [{ d: STAR, width: 1.1 }],
  "star-on": [{ d: STAR, width: 1.1, fill: true }],
  fold: [{ d: "M4.5 2.5 8 6 4.5 9.5" }],
  unfold: [{ d: "M2.5 4.5 6 8l3.5-3.5" }],
  remove: [{ d: "M3.5 3.5l5 5M8.5 3.5l-5 5" }],
  add: [{ d: "M6 2.5v7M2.5 6h7" }],
  minus: [{ d: "M2.5 6h7" }],
  fit: [{ d: "M2 4.5V2h2.5M7.5 2H10v2.5M10 7.5V10H7.5M4.5 10H2V7.5" }],
  find: [{ d: circle(5.25, 5.25, 3.25) }, { d: "M7.75 7.75 10.5 10.5" }],
  pane: [{ d: "M2.75 2.25h6.5a1 1 0 0 1 1 1v5.5a1 1 0 0 1-1 1h-6.5a1 1 0 0 1-1-1v-5.5a1 1 0 0 1 1-1zM4.75 2.25v7.5" }],
} satisfies Record<string, Shape[]>;

export type IconName = keyof typeof DRAWN;

export const ICONS: Record<IconName, Shape[]> = DRAWN;

/** A graph's light: the icon its node shows and the token of its one colour, the colour the
 * same status has in a list, a chip or a row. */
export const LIGHTS: Record<Light, { icon: IconName; colour: string }> = {
  green: { icon: "green", colour: "--green" },
  active: { icon: "running", colour: "--running" },
  waiting: { icon: "waiting", colour: "--queued" },
  red: { icon: "red", colour: "--red" },
  error: { icon: "error", colour: "--error" },
  blocked: { icon: "blocked", colour: "--blocked" },
  run: { icon: "run", colour: "--running" },
  reused: { icon: "reused", colour: "--reused" },
  checking: { icon: "checking", colour: "--checking" },
};

/** A status's icon: each its own silhouette. */
const STATUS: Record<Status, IconName> = {
  GREEN: "green",
  RED: "red",
  ERROR: "error",
  RUNNING: "running",
  WAITING_HUMAN: "human",
  DEFERRED: "human",
  YELLOW: "human",
  QUEUED: "queued",
  WAIT: "waiting",
  WAIT_DEPENDENCY: "waiting",
  BLOCKED: "blocked",
  BUDGET_EXHAUSTED: "blocked",
  PROVISIONAL: "notrun",
  STALE: "notrun",
  UNREVIEWED: "notrun",
  INCOMPLETE: "notrun",
  BASIS: "notrun",
};

export const statusIcon = (status: Status): IconName => STATUS[status];

/** The library's one-character marks, by meaning. */
const GLYPHS: Record<string, IconName> = {
  "✓": "green",
  "✗": "red",
  "!": "error",
  "◐": "running",
  "?": "human",
  "·": "queued",
  "…": "waiting",
  "⊘": "blocked",
  "○": "notrun",
  "◇": "notrun",
  "↻": "reused",
};

/** The icon of a row's or a chip's mark, given as the library's glyph and tone; `null` for the
 * two rare marks without one (a budget's `$`, an absent eval's `-`), which stay characters. On a
 * Current the tone tells what verify would do: would run, reused or still being checked. */
export function markIcon(glyph: string, tone: Tone | null): IconName | null {
  if (tone === "reused") return "reused";
  if (tone === "checking") return "checking";
  if (glyph === "○" && tone === "running") return "run";
  return GLYPHS[glyph] ?? null;
}
