// Wholes and their parts on a graph. An Artifact made of parts (the backend's `parts` on each
// node) is a whole, drawn as a box around its parts; a part that is a whole too is a box inside
// it. A part of several wholes is drawn once inside each of them: every drawing is a copy, with
// a key of its own, the keys of the wholes around it and then its tree id, `a:suite/a:app`. An
// Artifact that is no part of any is drawn once, keyed by its tree id, so a graph without parts
// draws as it always did.
//
// The layout places only the copies that are no wholes; a box is where its parts are, with room
// around them and above them for its name.
import type { GraphNode, Shape } from "../ipc/client";
import { NODE } from "./graph";

/** A box's room around its parts, and above them for its name. */
export const BOX = { pad: 16, header: 32 };

/** Where a copy is drawn, in graph coordinates. */
export type Rect = { x: number; y: number; width: number; height: number };

/** One drawing of an Artifact: its key, its node, the key of the box it is drawn in, if any,
 * and the keys of its parts' copies inside it, if it is a whole. */
export type Copy = { key: string; node: GraphNode; parent: string | null; parts: string[] };

/** Every copy of `graph`, each whole before its parts and the parts in the whole's order, the
 * Artifacts that are no parts in the graph's order. Parts close no cycle; one would be drawn
 * only once round. */
export function copies(graph: Shape): Copy[] {
  const byId = new Map(graph.nodes.map((node) => [node.id, node]));
  const parts = new Set(graph.nodes.flatMap((node) => node.parts.filter((part) => byId.has(part))));
  const drawn: Copy[] = [];
  const around = new Set<string>();
  const draw = (node: GraphNode, parent: Copy | null) => {
    const copy: Copy = { key: parent ? `${parent.key}/${node.id}` : node.id, node, parent: parent?.key ?? null, parts: [] };
    parent?.parts.push(copy.key);
    drawn.push(copy);
    around.add(node.id);
    for (const id of node.parts) {
      const part = byId.get(id);
      if (part && !around.has(id)) draw(part, copy);
    }
    around.delete(node.id);
  };
  for (const node of graph.nodes) if (!parts.has(node.id)) draw(node, null);
  return drawn;
}

/** The box of every copy of `drawn` the layout placed, in its order: a part's own at its
 * position, a whole's around its parts. */
export function boxes(drawn: Copy[], positions: Map<string, { x: number; y: number }>): Map<string, Rect> {
  const found = new Map<string, Rect>();
  // Parts come after their wholes: going backwards finds each whole's parts first.
  for (let at = drawn.length - 1; at >= 0; at -= 1) {
    const copy = drawn[at];
    if (copy.parts.length === 0) {
      const place = positions.get(copy.key);
      if (place) found.set(copy.key, { x: place.x, y: place.y, ...NODE });
      continue;
    }
    const inner = copy.parts.flatMap((key) => found.get(key) ?? []);
    if (inner.length === 0) continue;
    const { x, y, width, height } = around(inner);
    found.set(copy.key, { x: x - BOX.pad, y: y - BOX.header, width: width + 2 * BOX.pad, height: height + BOX.header + BOX.pad });
  }
  return new Map(drawn.flatMap((copy): [string, Rect][] => (found.has(copy.key) ? [[copy.key, found.get(copy.key)!]] : [])));
}

/** The box around `rects`, without a spread, which a large array overflows. */
export function around(rects: Iterable<Rect>): Rect {
  let [left, top, right, bottom] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const at of rects) {
    [left, top] = [Math.min(left, at.x), Math.min(top, at.y)];
    [right, bottom] = [Math.max(right, at.x + at.width), Math.max(bottom, at.y + at.height)];
  }
  return left === Infinity ? { x: 0, y: 0, width: 0, height: 0 } : { x: left, y: top, width: right - left, height: bottom - top };
}

/** The keys of each Artifact's copies by its tree id, in drawing order: the first is where
 * the find field goes. */
export function drawings(drawn: Copy[]): Map<string, string[]> {
  const found = new Map<string, string[]>();
  for (const copy of drawn) {
    const keys = found.get(copy.node.id);
    if (keys) keys.push(copy.key);
    else found.set(copy.node.id, [copy.key]);
  }
  return found;
}

/** Every edge of `graph` between copies: one from each copy of its source to each copy of its
 * target. */
export function copyEdges(graph: Shape, drawn: Copy[]): { source: string; target: string; edge: Shape["edges"][number] }[] {
  const keys = drawings(drawn);
  return graph.edges.flatMap((edge) =>
    (keys.get(edge.source) ?? []).flatMap((source) => (keys.get(edge.target) ?? []).map((target) => ({ source, target, edge }))),
  );
}
