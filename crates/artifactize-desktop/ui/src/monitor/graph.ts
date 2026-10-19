// The Run graph's frontend side: which lights mean what to a reader, and when a graph needs a
// new layout. The lights themselves come from the backend (`run_graph`), derived from the same
// states as the Run's tree of Artifacts and evals.
import type { Light, RunGraph } from "../ipc/client";

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
};

/** Legend order: as a Run goes, then what went wrong. */
export const LEGEND: Light[] = ["green", "active", "waiting", "red", "error", "blocked"];

/**
 * What a layout depends on: the Artifacts and the edges between them, never their lights. A
 * graph with the same shape keeps its node positions when its lights change.
 */
export function shape(graph: Pick<RunGraph, "nodes" | "edges">): string {
  const nodes = graph.nodes.map((node) => node.id).sort();
  const edges = graph.edges.map((edge) => `${edge.source}>${edge.target}`).sort();
  return JSON.stringify([nodes, edges]);
}

export type Point = { x: number; y: number };
