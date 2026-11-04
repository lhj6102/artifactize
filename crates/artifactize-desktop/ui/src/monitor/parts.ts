// Wholes and their parts on a graph. An Artifact made of parts (the backend's `parts` on each
// node) is a whole, drawn as a box around its parts; a part that is a whole too is a box inside
// it. A part of several wholes is drawn once inside each of them: every drawing is a copy, with
// a key of its own, the keys of the wholes around it and then its tree id, `a:suite/a:app`. An
// Artifact that is no part of any is drawn once, keyed by its tree id, so a graph without parts
// draws as it always did.
//
// A minimized whole is drawn as a node instead, its parts hidden: it stands for every Artifact
// it hides, so their edges meet it, those between the same two copies joined into one, lit by
// the library's precedence of lights.
//
// The layout places only the copies that are no boxes; a box is where its parts are, with room
// around them and above them for its name.
import type { GraphNode, Light, Shape } from "../ipc/client";
import { NODE } from "./graph";

/** A box's room around its parts, and above them for its name. */
export const BOX = { pad: 16, header: 32 };

/** Where a copy is drawn, in graph coordinates. */
export type Rect = { x: number; y: number; width: number; height: number };

/** One drawing of an Artifact: its key, its node, the key of the box it is drawn in, if any,
 * and the keys of its parts' copies inside it, if it is a whole drawn as a box. A minimized
 * whole draws none of its parts: `hides` names every Artifact inside it, as deep as its parts
 * go; `count` is how many parts a whole has, drawn or not. */
export type Copy = { key: string; node: GraphNode; parent: string | null; parts: string[]; minimized: boolean; count: number; hides: string[] };

/** Every copy of `graph`, each whole before its parts and the parts in the whole's order, the
 * Artifacts that are no parts in the graph's order; the wholes `minimized` names by tree id
 * drawn as nodes, without their parts. Parts close no cycle; one would be drawn only once round. */
export function copies(graph: Shape, minimized: readonly string[] = []): Copy[] {
  const folding = new Set(minimized);
  const byId = new Map(graph.nodes.map((node) => [node.id, node]));
  const parts = new Set(graph.nodes.flatMap((node) => node.parts.filter((part) => byId.has(part))));
  const drawn: Copy[] = [];
  const around = new Set<string>();
  /** The parts of `node` the graph has that close no cycle. */
  const inside = (node: GraphNode) => node.parts.flatMap((id) => (byId.has(id) && !around.has(id) ? [byId.get(id)!] : []));
  /** Every Artifact inside `node`, as deep as its parts go, added to `found`. */
  const hidden = (node: GraphNode, found: Set<string>) => {
    around.add(node.id);
    for (const part of inside(node)) {
      found.add(part.id);
      hidden(part, found);
    }
    around.delete(node.id);
  };
  const draw = (node: GraphNode, parent: Copy | null) => {
    const own = inside(node);
    const copy: Copy = {
      key: parent ? `${parent.key}/${node.id}` : node.id,
      node,
      parent: parent?.key ?? null,
      parts: [],
      minimized: own.length > 0 && folding.has(node.id),
      count: own.length,
      hides: [],
    };
    parent?.parts.push(copy.key);
    drawn.push(copy);
    if (copy.minimized) {
      const found = new Set<string>();
      hidden(node, found);
      copy.hides = [...found];
      return;
    }
    around.add(node.id);
    for (const part of own) draw(part, copy);
    around.delete(node.id);
  };
  for (const node of graph.nodes) if (!parts.has(node.id)) draw(node, null);
  return drawn;
}

/** How many parts a minimized whole says it has. */
export const partsCount = (count: number): string => (count === 1 ? "1 part" : `${count} parts`);

/** The wholes of `graph` that `minimized` names, in the graph's order: those its copies draw
 * as nodes. */
export function folded(graph: Shape, minimized: readonly string[]): string[] {
  const folding = new Set(minimized);
  return graph.nodes.filter((node) => node.parts.length > 0 && folding.has(node.id)).map((node) => node.id);
}

/** The box of every copy of `drawn` the layout placed, in its order: a node's own at its
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

/** The keys of the copies that draw each Artifact, by its tree id, in drawing order: its own
 * copies and the minimized wholes that hide it. The first is where the find field goes. */
export function drawings(drawn: Copy[]): Map<string, string[]> {
  const found = new Map<string, string[]>();
  const add = (id: string, key: string) => {
    const keys = found.get(id);
    if (!keys) found.set(id, [key]);
    else if (!keys.includes(key)) keys.push(key);
  };
  for (const copy of drawn) {
    add(copy.node.id, copy.key);
    for (const id of copy.hides) add(id, copy.key);
  }
  return found;
}

/** An edge between two copies, lit as the edge of `graph` it stands for. */
export type CopyEdge = { source: string; target: string; edge: Shape["edges"][number] };

/** Every edge of `graph` between copies: from each copy that draws its source to each copy that
 * draws its target, none within one minimized whole, and the edges between the same two copies
 * once, in the light of theirs that comes first in `order`, the graph's precedence the library
 * gives (`PRECEDENCE`). */
export function copyEdges(graph: Shape, drawn: Copy[], order: readonly Light[]): CopyEdge[] {
  const keys = drawings(drawn);
  const joined = new Map<string, CopyEdge>();
  for (const edge of graph.edges) {
    for (const source of keys.get(edge.source) ?? []) {
      for (const target of keys.get(edge.target) ?? []) {
        if (source === target) continue;
        const pair = `${source}>${target}`;
        const known = joined.get(pair);
        if (!known || order.indexOf(edge.light) < order.indexOf(known.edge.light)) joined.set(pair, { source, target, edge });
      }
    }
  }
  return [...joined.values()];
}
