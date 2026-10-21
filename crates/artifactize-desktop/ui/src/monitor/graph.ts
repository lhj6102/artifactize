// The graphs' frontend side: which lights mean what to a reader, and when a graph needs a new
// layout. The lights themselves come from the backend (`graph`), derived from the same states
// as the Run's tree of Artifacts and evals, or from what verify would do on a Current.
import type { Graph, Light } from "../ipc/client";

/** Above this many Artifacts the graph asks before laying out. */
export const LIMIT = 300;

/** Words for each light, for labels and the legend; colours are `light-<name>` classes. */
export const WORDS: Record<Light, string> = {
  green: "GREEN",
  active: "in progress",
  waiting: "waiting",
  red: "RED",
  error: "ERROR",
  blocked: "blocked by upstream",
  run: "would run",
  reused: "reused",
};

/** Legend order: as a Run goes, then what went wrong. */
export const LEGEND: Light[] = ["green", "active", "waiting", "red", "error", "blocked"];

/** A Current's legend: what verify would do with each Artifact now. */
export const CURRENT_LEGEND: Light[] = ["reused", "run", "waiting", "blocked"];

/**
 * What a layout depends on: the Artifacts and the edges between them, never their lights. A
 * graph with the same shape keeps its node positions when its lights change.
 */
export function shape(graph: Pick<Graph, "nodes" | "edges">): string {
  const nodes = graph.nodes.map((node) => node.id).sort();
  const edges = graph.edges.map((edge) => `${edge.source}>${edge.target}`).sort();
  return JSON.stringify([nodes, edges]);
}

export type Point = { x: number; y: number };
