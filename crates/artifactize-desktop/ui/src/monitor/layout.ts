// Node positions for a graph of Artifacts, layered left to right, upstream first.
//
// A graph of at most `ELK_LIMIT` Artifacts is laid out by ELK's layered algorithm, which
// widens the gap between two layers with the edges crossing it and gives long edges their own
// lanes. ELK runs in a Web Worker started on first use, so its layout never blocks the window,
// and neither a larger graph nor the tree view ever loads it.
//
// A larger graph (a catalog Run of tens of thousands of Artifacts) gets `layered`, in time
// about linear in the graph's size, so that it lays out at once: each connected part of the
// graph gets its own layers, an Artifact one layer right of the furthest Artifact it uses, and
// a few sweeps order every layer by where its neighbours sit. The parts are then packed in rows
// into a box about as wide as a screen.
//
// In both, an Artifact that a great many others use (a shared input, such as one criteria file
// every review reads) would tie every part into one tall, narrow column, and makes ELK slow; it
// stands apart in its own column before the rest instead, its edges still drawn to every
// Artifact that uses it.
import type { Shape } from "../ipc/client";
import type { Point } from "./graph";

/** Every Artifact node's box; names longer than fit are cut with an ellipsis. */
export const NODE = { width: 168, height: 44 };
/** The most Artifacts ELK lays out, in about half a second for a real graph this size. A larger
 * graph gets `layered`, which takes milliseconds at any size. */
export const ELK_LIMIT = 1000;

/** Room between layers, and between the Artifacts of one layer. */
const GAP = { x: 72, y: 20 };
/** Room between the packed parts of the graph. */
const APART = 40;
/** An Artifact used by more than this many is a shared input. A part of a real graph has a few
 * dozen Artifacts, and none uses one Artifact more than about ten times. */
const SHARED = 64;
/** Ordering sweeps, each down and back up the layers: crossings settle within a few. */
const SWEEPS = 4;
/** The packed box's width to height, about a screen's. */
const ASPECT = 16 / 10;

/** A laid-out part: its Artifacts, each one's cell (column, row), and its size in cells. */
type Part = { members: number[]; column: Int32Array; row: Float64Array; columns: number; rows: number };

/** The largest of `values`, or 0; a spread of a large array would overflow the stack. */
function largest(values: ArrayLike<number>): number {
  let top = 0;
  for (let at = 0; at < values.length; at += 1) top = Math.max(top, values[at]);
  return top;
}

/** Top-left positions of `graph`'s nodes: by ELK for at most `ELK_LIMIT` Artifacts, else by
 * `layered`. */
export function layout(graph: Shape): Promise<Map<string, Point>> {
  return graph.nodes.length > ELK_LIMIT ? Promise.resolve(layered(graph)) : elk(graph);
}

/** Each edge once, between two different Artifacts the graph has, by their numbers in
 * `graph.nodes`, and which Artifacts are shared inputs. */
function links(graph: Shape) {
  const count = graph.nodes.length;
  const index = new Map(graph.nodes.map((node, at) => [node.id, at]));
  const pairs = new Set<number>();
  const from: number[] = [];
  const to: number[] = [];
  for (const edge of graph.edges) {
    const source = index.get(edge.source);
    const target = index.get(edge.target);
    if (source === undefined || target === undefined || source === target) continue;
    const key = source * count + target;
    if (pairs.has(key)) continue;
    pairs.add(key);
    from.push(source);
    to.push(target);
  }
  const uses = new Int32Array(count);
  for (const source of from) uses[source] += 1;
  const shared = Array.from(uses, (used) => used > SHARED);
  return { count, from, to, shared };
}

type Elk = { layout: (graph: object) => Promise<{ children?: { id: string; x?: number; y?: number }[] }> };
let worker: Promise<Elk> | null = null;

/** ELK's layout of `graph`, with the options the Graph has always given it; the shared inputs
 * and their edges are left out of it and stand in a column on its left. */
async function elk(graph: Shape): Promise<Map<string, Point>> {
  worker ??= Promise.all([import("elkjs/lib/elk-api.js"), import("elkjs/lib/elk-worker.min.js?url")]).then(
    ([{ default: ELK }, { default: workerUrl }]) => new ELK({ workerUrl }) as Elk,
  );
  const { shared } = links(graph);
  const inputs = new Set(graph.nodes.filter((_, at) => shared[at]).map((node) => node.id));
  const laid = await (
    await worker
  ).layout({
    id: "run",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.layered.spacing.nodeNodeBetweenLayers": String(GAP.x),
      "elk.spacing.nodeNode": String(GAP.y),
      "elk.spacing.componentComponent": String(APART),
      // Keep the tree's order (upstream first) within a layer where crossings allow. With the
      // shared inputs left out it costs little: 3-15% more than without it at 1,000 Artifacts.
      "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
    },
    children: graph.nodes.filter((node) => !inputs.has(node.id)).map((node) => ({ id: node.id, ...NODE })),
    edges: graph.edges
      .filter((edge) => !inputs.has(edge.source) && !inputs.has(edge.target))
      .map((edge, index) => ({ id: `e${index}`, sources: [edge.source], targets: [edge.target] })),
  });
  const left = inputs.size > 0 ? NODE.width + GAP.x + APART : 0;
  return new Map([
    ...[...inputs].map((id, row): [string, Point] => [id, { x: 0, y: row * (NODE.height + GAP.y) }]),
    ...(laid.children ?? []).map((child): [string, Point] => [child.id, { x: left + (child.x ?? 0), y: child.y ?? 0 }]),
  ]);
}

/** Top-left positions of `graph`'s nodes, layered and packed in time about linear in its size. */
export function layered(graph: Shape): Map<string, Point> {
  const { count, from, to, shared } = links(graph);

  // The connected parts, apart from shared inputs and their edges.
  const parent = Int32Array.from({ length: count }, (_, at) => at);
  const root = (at: number): number => {
    while (parent[at] !== at) {
      parent[at] = parent[parent[at]];
      at = parent[at];
    }
    return at;
  };
  const before = Array.from({ length: count }, () => [] as number[]);
  const after = Array.from({ length: count }, () => [] as number[]);
  for (let at = 0; at < from.length; at += 1) {
    if (shared[from[at]] || shared[to[at]]) continue;
    after[from[at]].push(to[at]);
    before[to[at]].push(from[at]);
    const [one, other] = [root(from[at]), root(to[at])];
    if (one !== other) parent[Math.max(one, other)] = Math.min(one, other);
  }
  const groups = new Map<number, number[]>();
  const inputs: number[] = [];
  for (let at = 0; at < count; at += 1) {
    if (shared[at]) inputs.push(at);
    else {
      const group = groups.get(root(at));
      if (group) group.push(at);
      else groups.set(root(at), [at]);
    }
  }

  // Parts in the order of their first Artifact: upstream first, as the tree lists them.
  const parts = [...groups.values()].map((members) => layer(members, before, after));
  return pack(inputs, parts, graph);
}

/** Lay out one connected part in layers. */
function layer(members: number[], before: number[][], after: number[][]): Part {
  const local = new Map(members.map((node, at) => [node, at]));
  const size = members.length;
  const near = (lists: number[][]) => members.map((node) => lists[node].map((other) => local.get(other)!));
  const [uses, used] = [near(before), near(after)];
  // Edges that close a cycle point back; the rest go forward. A depth-first walk in the tree's
  // order finds them: an edge to an Artifact still on the walk's path closes a cycle.
  const state = new Uint8Array(size);
  const back = new Set<number>();
  for (let start = 0; start < size; start += 1) {
    if (state[start] !== 0) continue;
    const path: [number, number][] = [[start, 0]];
    state[start] = 1;
    while (path.length > 0) {
      const top = path[path.length - 1];
      const target = used[top[0]][top[1]];
      if (target === undefined) {
        state[top[0]] = 2;
        path.pop();
        continue;
      }
      top[1] += 1;
      if (state[target] === 1) back.add(top[0] * size + target);
      else if (state[target] === 0) {
        state[target] = 1;
        path.push([target, 0]);
      }
    }
  }
  const inputs = uses.map((sources, at) => sources.filter((source) => !back.has(source * size + at)));
  const outputs = used.map((targets, at) => targets.filter((target) => !back.has(at * size + target)));

  // Each Artifact one layer right of the furthest one it uses, in topological order.
  const waiting = Int32Array.from(inputs, (sources) => sources.length);
  const order: number[] = [];
  for (let at = 0; at < size; at += 1) if (waiting[at] === 0) order.push(at);
  const column = new Int32Array(size);
  for (let next = 0; next < order.length; next += 1) {
    const at = order[next];
    for (const target of outputs[at]) {
      column[target] = Math.max(column[target], column[at] + 1);
      waiting[target] -= 1;
      if (waiting[target] === 0) order.push(target);
    }
  }
  // An Artifact that uses nothing sits just before the first Artifact that uses it.
  for (let next = order.length - 1; next >= 0; next -= 1) {
    const at = order[next];
    if (inputs[at].length > 0 || outputs[at].length === 0) continue;
    column[at] = outputs[at].reduce((first, target) => Math.min(first, column[target]), Infinity) - 1;
  }
  const columns = largest(column) + 1;
  const layers = Array.from({ length: columns }, () => [] as number[]);
  for (let at = 0; at < size; at += 1) layers[column[at]].push(at);

  // Order each layer by where its neighbours sit, sweeping down and back up the layers; a
  // layer shorter than the part's tallest is centred on it.
  const rows = largest(layers.map((each) => each.length));
  const row = new Float64Array(size);
  const place = (each: number[]) => {
    const offset = Math.floor((rows - each.length) / 2);
    each.forEach((at, position) => (row[at] = offset + position));
  };
  layers.forEach(place);
  const sort = (each: number[], neighbours: number[][]) => {
    const key = (at: number) => {
      const near = neighbours[at];
      return near.length === 0 ? row[at] : near.reduce((sum, other) => sum + row[other], 0) / near.length;
    };
    const keyed = each.map((at) => ({ at, key: key(at) }));
    keyed.sort((one, other) => one.key - other.key || row[one.at] - row[other.at]);
    keyed.forEach(({ at }, position) => (each[position] = at));
    place(each);
  };
  for (let sweep = 0; sweep < SWEEPS; sweep += 1) {
    for (let at = 1; at < columns; at += 1) sort(layers[at], inputs);
    for (let at = columns - 2; at >= 0; at -= 1) sort(layers[at], outputs);
  }
  return { members, column, row, columns, rows };
}

/** Place the shared inputs in a column on the left and pack the parts in rows right of it, in
 * their order, into a box about `ASPECT` wide; then place each Artifact in its part. */
function pack(inputs: number[], parts: Part[], graph: Shape): Map<string, Point> {
  const step = { x: NODE.width + GAP.x, y: NODE.height + GAP.y };
  const placed = new Map<string, Point>();
  inputs.forEach((node, row) => placed.set(graph.nodes[node].id, { x: 0, y: row * step.y }));
  const left = inputs.length > 0 ? NODE.width + GAP.x + APART : 0;
  const box = (part: Part) => ({ width: part.columns * step.x - GAP.x, height: part.rows * step.y - GAP.y });
  const area = parts.reduce((sum, part) => sum + (box(part).width + APART) * (box(part).height + APART), 0);
  const width = Math.max(Math.sqrt(area * ASPECT), largest(parts.map((part) => box(part).width)));
  let [x, y, tallest] = [0, 0, 0];
  for (const part of parts) {
    const { width: wide, height: high } = box(part);
    if (x > 0 && x + wide > width) [x, y, tallest] = [0, y + tallest + APART, 0];
    part.members.forEach((node, at) => {
      placed.set(graph.nodes[node].id, { x: left + x + part.column[at] * step.x, y: y + part.row[at] * step.y });
    });
    x += wide + APART;
    tallest = Math.max(tallest, high);
  }
  return placed;
}
