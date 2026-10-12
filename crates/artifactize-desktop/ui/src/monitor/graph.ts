// The Run graph's frontend side: which lights mean what to a reader, when a graph needs a new
// layout, and which connected node an arrow key moves to. The lights themselves come from the
// backend (`run_graph`), derived from the same states as the tree.
import type { Light, RunGraph } from "../ipc/client";

/** Above this many Artifacts the graph asks before laying out; the tree shows any Run. */
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
export function shape(graph: RunGraph): string {
  const nodes = graph.nodes.map((node) => node.id).sort();
  const edges = graph.edges.map((edge) => `${edge.source}>${edge.target}`).sort();
  return JSON.stringify([nodes, edges]);
}

export type Point = { x: number; y: number };
export type Arrow = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown";

/**
 * The node an arrow key moves to from `id` in a left-to-right layout: → the vertically nearest
 * downstream Artifact (the upper one of two as near), ← the nearest upstream one, ↑/↓ the
 * nearest node above or below in the same column that shares an upstream or downstream
 * Artifact with it (any node of the column when none does). `null` when there is none.
 */
export function neighbor(graph: RunGraph, at: Map<string, Point>, id: string, key: Arrow): string | null {
  const here = at.get(id);
  if (!here) return null;
  const downstream = (node: string) => graph.edges.filter((edge) => edge.source === node).map((edge) => edge.target);
  const upstream = (node: string) => graph.edges.filter((edge) => edge.target === node).map((edge) => edge.source);
  const nearest = (ids: string[], score: (point: Point) => number) =>
    ids
      .filter((other) => other !== id && at.has(other))
      .map((other) => ({ other, point: at.get(other)!, score: score(at.get(other)!) }))
      .filter(({ score }) => Number.isFinite(score))
      // Ties go to the upper one, then by id.
      .sort((a, b) => a.score - b.score || a.point.y - b.point.y || a.other.localeCompare(b.other))[0]?.other ?? null;
  const vertical = (point: Point) => Math.abs(point.y - here.y);
  if (key === "ArrowRight") return nearest(downstream(id), vertical);
  if (key === "ArrowLeft") return nearest(upstream(id), vertical);
  const column = [...at.entries()].filter(([, point]) => Math.abs(point.x - here.x) < 1).map(([other]) => other);
  const related = new Set([...upstream(id).flatMap(downstream), ...downstream(id).flatMap(upstream)]);
  const siblings = column.filter((other) => related.has(other));
  const pool = siblings.length > 0 ? siblings : column;
  const below = key === "ArrowDown";
  return nearest(pool, (point) => {
    const distance = below ? point.y - here.y : here.y - point.y;
    return distance > 0 ? distance : Infinity;
  });
}
