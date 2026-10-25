// Spaces: Artifacts named `space::name` are drawn together in their space's box. What a
// name's space is comes from the library (`space` on each node); this only groups the nodes
// and finds each box around its laid-out Artifacts.
import type { Shape } from "../ipc/client";
import { NODE, type Point } from "./graph";

/** A space's box: room around its Artifacts, and above them for the space's name. */
export const SPACE = { pad: 16, header: 28 };

/** A space's box around its Artifacts, in graph coordinates. */
export type SpaceBox = { space: string; x: number; y: number; width: number; height: number };

/** The box around the Artifacts at `points`, without a spread, which a large array overflows. */
export function extent(points: Iterable<Point>) {
  let [left, top, right, bottom] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const at of points) {
    [left, top] = [Math.min(left, at.x), Math.min(top, at.y)];
    [right, bottom] = [Math.max(right, at.x + NODE.width), Math.max(bottom, at.y + NODE.height)];
  }
  return { left, top, width: right - left, height: bottom - top };
}

/** `graph`'s Artifacts by space, the groups in the order of their first Artifact. */
export function spaces(graph: Shape): { space: string | null; nodes: Shape["nodes"] }[] {
  const groups = new Map<string | null, Shape["nodes"]>();
  for (const node of graph.nodes) {
    const nodes = groups.get(node.space);
    if (nodes) nodes.push(node);
    else groups.set(node.space, [node]);
  }
  return [...groups].map(([space, nodes]) => ({ space, nodes }));
}

/** The box of each space in `graph` around its Artifacts at `positions`. */
export function boxes(graph: Shape, positions: Map<string, Point>): SpaceBox[] {
  return spaces(graph).flatMap(({ space, nodes }) => {
    const all = nodes.flatMap((node) => positions.get(node.id) ?? []);
    if (space === null || all.length === 0) return [];
    const { left, top, width, height } = extent(all);
    return [{ space, x: left - SPACE.pad, y: top - SPACE.header, width: width + 2 * SPACE.pad, height: height + SPACE.header + SPACE.pad }];
  });
}
