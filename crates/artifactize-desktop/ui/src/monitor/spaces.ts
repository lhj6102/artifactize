// Spaces: Artifacts named `space::name` are drawn together in their space's region. What a
// name's space is comes from the library (`space` on each node); this only groups the nodes
// and finds each region around its laid-out Artifacts.
//
// A region is no plain box: its outline is the hull around its Artifacts, with room around
// them and, above the topmost, for the space's name, its corners rounded by that room. It
// follows its Artifacts wherever the layout puts them, and stays within the box the layout
// keeps for the space.
import type { Shape } from "../ipc/client";
import { NODE, type Point } from "./graph";

/** A space's region: room around its Artifacts, also the radius of its corners, and above them
 * for the space's name. */
export const SPACE = { pad: 20, header: 32 };

/** A space's region around its Artifacts. Its box (`x`, `y`, `width`, `height`) is in graph
 * coordinates; its outline, an SVG path, and the strip of its label are from the box's corner. */
export type Region = {
  space: string;
  x: number;
  y: number;
  width: number;
  height: number;
  outline: string;
  label: { x: number; width: number };
};

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

/** The region of each space in `graph` around its Artifacts at `positions`. */
export function regions(graph: Shape, positions: Map<string, Point>): Region[] {
  return spaces(graph).flatMap(({ space, nodes }) => {
    const all = nodes.flatMap((node) => positions.get(node.id) ?? []);
    return space === null || all.length === 0 ? [] : [region(space, all)];
  });
}

/** The region of `space` around its Artifacts at `points`. */
function region(space: string, points: Point[]): Region {
  // The label stands above the topmost Artifact, the leftmost of those as high.
  const top = points.reduce((first, at) => (at.y < first.y || (at.y === first.y && at.x < first.x) ? at : first));
  const corners = points.flatMap((at) => {
    const high = at === top ? at.y - SPACE.header + SPACE.pad : at.y;
    const [right, bottom] = [at.x + NODE.width, at.y + NODE.height];
    return [{ x: at.x, y: high }, { x: right, y: high }, { x: right, y: bottom }, { x: at.x, y: bottom }];
  });
  const { left, top: high, width, height } = extent(points);
  const corner = { x: left - SPACE.pad, y: high - SPACE.header };
  return {
    space,
    ...corner,
    width: width + 2 * SPACE.pad,
    height: height + SPACE.header + SPACE.pad,
    outline: rounded(hull(corners).map((at) => ({ x: at.x - corner.x, y: at.y - corner.y })), SPACE.pad),
    label: { x: top.x - corner.x, width: NODE.width },
  };
}

/** The convex hull of `points`, clockwise on the screen (y down), without collinear points. */
function hull(points: Point[]): Point[] {
  const sorted = [...points].sort((one, other) => one.x - other.x || one.y - other.y);
  const turn = (o: Point, a: Point, b: Point) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const half = (from: Point[]) => {
    const chain: Point[] = [];
    for (const at of from) {
      while (chain.length >= 2 && turn(chain[chain.length - 2], chain[chain.length - 1], at) <= 0) chain.pop();
      chain.push(at);
    }
    chain.pop();
    return chain;
  };
  return [...half(sorted), ...half(sorted.reverse())];
}

/** An SVG path around the clockwise convex polygon `points`, `radius` outside every edge, its
 * corners arcs of that radius. */
function rounded(points: Point[], radius: number): string {
  const normal = (from: Point, to: Point) => {
    const length = Math.hypot(to.x - from.x, to.y - from.y);
    return { x: (to.y - from.y) / length, y: (from.x - to.x) / length };
  };
  const out = (at: Point, by: Point) => `${at.x + radius * by.x} ${at.y + radius * by.y}`;
  const normals = points.map((at, index) => normal(at, points[(index + 1) % points.length]));
  const steps = points.map((_, index) => {
    const [to, next] = [points[(index + 1) % points.length], normals[(index + 1) % points.length]];
    return `L ${out(to, normals[index])} A ${radius} ${radius} 0 0 1 ${out(to, next)}`;
  });
  return `M ${out(points[0], normals[0])} ${steps.join(" ")} Z`;
}
