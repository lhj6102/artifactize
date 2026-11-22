// The graphs' frontend side: which lights mean what to a reader, and how a known shape takes
// new lights. The lights themselves come from the backend (`graph`), derived from the same
// states as the Run's tree of Artifacts and evals, or from what verify would do on a Current.
import type { Light, Shape, Tally, Update } from "../ipc/client";

/** One user-requested camera zoom step, on the strip and in the palette. */
export const ZOOM_STEP = 1.2;

/** Words for each light, for labels and Help; colours are `light-<name>` classes and icons
 * are icons.ts's. */
export const WORDS: Record<Light, string> = {
  green: "GREEN",
  active: "in progress",
  waiting: "waiting",
  red: "RED",
  error: "ERROR",
  blocked: "blocked by upstream",
  run: "would run",
  reused: "reused",
  checking: "checking",
};

/** A Run's marks as Help lists them: as a Run goes, then what went wrong. */
export const LEGEND: Light[] = ["green", "active", "waiting", "red", "error", "blocked"];

/** A Current's marks: what verify would do with each Artifact now. */
export const CURRENT_LEGEND: Light[] = ["reused", "run", "waiting", "blocked", "checking"];

/** The lights whose edges take their colour: what holds a blocked Artifact shows; every other
 * edge is neutral. */
export const BLOCKING: ReadonlySet<Light> = new Set<Light>(["red", "error", "blocked"]);

/** A Run's passing evals; a Current uses the library's own plan note, not this tally. */
export function tallyWords(tally: Tally | null): string {
  return tally === null ? "" : `, ${tally.passed} of ${tally.of} evals GREEN`;
}

/** A Current with no plan note can still name its observed mark. */
export function currentWords(light: Light): string { return WORDS[light]; }

export type Point = { x: number; y: number };

/** Every Artifact node's box; names longer than fit are cut with an ellipsis. */
export const NODE = { width: 168, height: 44 };

/** `shape` with the lights and tallies of `update`, one per node and a light per edge in its
 * order; `shape` itself when none changed, so that a view of it need not paint again. */
export function relight(shape: Shape, update: Extract<Update, { kind: "lights" }>): Shape {
  const tally = (at: number, node: Shape["nodes"][number]) => (at < update.tallies.length ? update.tallies[at] : node.tally);
  const same = shape.nodes.every((node, at) => (update.nodes[at] ?? node.light) === node.light && sameTally(tally(at, node), node.tally))
    && shape.edges.every((edge, at) => (update.edges[at] ?? edge.light) === edge.light);
  if (same) return shape;
  return {
    nodes: shape.nodes.map((node, at) => ({ ...node, light: update.nodes[at] ?? node.light, tally: tally(at, node) })),
    edges: shape.edges.map((edge, at) => ({ ...edge, light: update.edges[at] ?? edge.light })),
  };
}

function sameTally(one: Tally | null, other: Tally | null): boolean {
  return one === other || (one !== null && other !== null && one.passed === other.passed && one.of === other.of);
}
