// The Run graph's frontend side: which lights mean what to a reader, and how a known shape
// takes new lights. The lights themselves come from the backend (`run_graph`), derived from the
// same states as the Run's tree of Artifacts and evals.
import type { Light, Shape, Update } from "../ipc/client";

/** Words for each light, for labels and the legend; colours are `light-<name>` classes. */
export const WORDS: Record<Light, string> = {
  green: "GREEN",
  active: "in progress",
  waiting: "waiting",
  red: "RED",
  error: "ERROR",
  blocked: "blocked by upstream",
};

/** Legend order: as a Run goes, then what went wrong. */
export const LEGEND: Light[] = ["green", "active", "waiting", "red", "error", "blocked"];

export type Point = { x: number; y: number };

/** `shape` with the lights of `update`, one per node and per edge in its order; `shape` itself
 * when none changed, so that a view of it need not paint again. */
export function relight(shape: Shape, update: Extract<Update, { kind: "lights" }>): Shape {
  const same = shape.nodes.every((node, at) => (update.nodes[at] ?? node.light) === node.light)
    && shape.edges.every((edge, at) => (update.edges[at] ?? edge.light) === edge.light);
  if (same) return shape;
  return {
    nodes: shape.nodes.map((node, at) => ({ ...node, light: update.nodes[at] ?? node.light })),
    edges: shape.edges.map((edge, at) => ({ ...edge, light: update.edges[at] ?? edge.light })),
  };
}
