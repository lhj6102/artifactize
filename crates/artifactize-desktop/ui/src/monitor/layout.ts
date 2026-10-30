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
//
// A whole's parts are laid out together inside its box (parts.ts), as a graph of their own: of
// the copies drawn directly in it, a whole among them as large as its box, and the edges
// between them, an edge between parts of different copies standing for the copies that hold
// them. The graph around is then laid out with each box as one Artifact of its size. A graph
// without parts is laid out as one, as before.
import type { Shape } from "../ipc/client";
import { NODE, type Point } from "./graph";
import { BOX, around, copies, type Copy } from "./parts";

export { NODE };
/** The most Artifacts ELK lays out, in about half a second for a real graph this size. A larger
 * graph gets `layered`, which takes milliseconds at any size. */
export const ELK_LIMIT = 1000;

/** Room between layers (`x`), between the Artifacts of one layer (`y`), and between the packed
 * parts of the graph (`apart`). */
type Room = { x: number; y: number; apart: number };
/** A graph's room. */
const ROOM: Room = { x: 72, y: 20, apart: 40 };
/** An Artifact used by more than this many is a shared input. A part of a real graph has a few
 * dozen Artifacts, and none uses one Artifact more than about ten times. */
const SHARED = 64;
/** Ordering sweeps, each down and back up the layers: crossings settle within a few. */
const SWEEPS = 4;
/** The packed box's width to height, about a screen's. */
const ASPECT = 16 / 10;

/** A laid-out part: its Artifacts, each one's cell (column, row), and its size in cells. */
type Part = { members: number[]; column: Int32Array; row: Float64Array; columns: number; rows: number };

/** An Artifact's size on the graph. */
type Size = { width: number; height: number };

/** A graph to lay out: its Artifacts by id, the size of each that is not `NODE`'s, and its
 * edges. A graph's `Shape` is one. */
type Level = { nodes: { id: string }[]; edges: { source: string; target: string }[]; sizes?: Map<string, Size> };

const size = (level: Level, id: string): Size => level.sizes?.get(id) ?? NODE;

/** The largest of `values`, or 0; a spread of a large array would overflow the stack. */
function largest(values: ArrayLike<number>): number {
  let top = 0;
  for (let at = 0; at < values.length; at += 1) top = Math.max(top, values[at]);
  return top;
}

/** Top-left positions of `graph`'s copies that are no wholes, by key: by ELK while at most
 * `ELK_LIMIT` copies are drawn, else by `layered`; each whole's parts inside its box. */
export async function layout(graph: Shape): Promise<Map<string, Point>> {
  const drawn = copies(graph);
  const flat = drawn.length > ELK_LIMIT ? (level: Level) => Promise.resolve(layered(level, ROOM)) : (level: Level) => elk(level, ROOM);
  if (drawn.every((copy) => copy.parts.length === 0)) return flat(graph);
  return nest(graph, drawn, flat);
}

/** `drawn` laid out level by level with `flat`: each whole's parts inside it, the deepest
 * first, then the copies drawn in no whole. */
async function nest(graph: Shape, drawn: Copy[], flat: (level: Level) => Promise<Map<string, Point>>): Promise<Map<string, Point>> {
  const byKey = new Map(drawn.map((copy) => [copy.key, copy]));
  const keys = new Map<string, string[]>();
  for (const copy of drawn) keys.set(copy.node.id, [...(keys.get(copy.node.id) ?? []), copy.key]);
  /** The keys of the boxes around `key`, the outermost first, then `key`. */
  const chain = (key: string) => {
    const found = [key];
    for (let at = byKey.get(key)!.parent; at !== null; at = byKey.get(at)!.parent) found.unshift(at);
    return found;
  };
  // Each edge between copies, at the level of the box around both: between the copies drawn
  // directly in it that hold either end.
  const edges = new Map<string | null, Set<string>>();
  for (const edge of graph.edges) {
    for (const source of keys.get(edge.source) ?? []) {
      for (const target of keys.get(edge.target) ?? []) {
        const [from, to] = [chain(source), chain(target)];
        let at = 0;
        while (at < from.length && at < to.length && from[at] === to[at]) at += 1;
        if (at === from.length || at === to.length) continue;
        const level = at === 0 ? null : from[at - 1];
        const pairs = edges.get(level) ?? new Set<string>();
        pairs.add(`${from[at]}\n${to[at]}`);
        edges.set(level, pairs);
      }
    }
  }
  // Each box's parts by their place in it, its top-left corner at (0, 0); and its size.
  const inside = new Map<string, Map<string, Point>>();
  const sizes = new Map<string, Size>();
  const lay = async (members: Copy[], level: string | null) => {
    const own = new Map(members.map((copy) => [copy.key, copy]));
    await Promise.all(members.filter((copy) => copy.parts.length > 0).map((copy) => lay(copy.parts.map((key) => byKey.get(key)!), copy.key)));
    const pairs = [...(edges.get(level) ?? [])].map((pair) => pair.split("\n"));
    const placed =
      members.length === 1
        ? new Map([[members[0].key, { x: 0, y: 0 }]])
        : await flat({
            nodes: members.map((copy) => ({ id: copy.key })),
            edges: pairs.filter(([source, target]) => own.has(source) && own.has(target)).map(([source, target]) => ({ source, target })),
            sizes,
          });
    if (level === null) return placed;
    const box = around([...placed].map(([key, at]) => ({ ...at, ...(sizes.get(key) ?? NODE) })));
    inside.set(level, new Map([...placed].map(([key, at]) => [key, { x: at.x - box.x + BOX.pad, y: at.y - box.y + BOX.header }])));
    sizes.set(level, { width: box.width + 2 * BOX.pad, height: box.height + BOX.header + BOX.pad });
    return placed;
  };
  const top = await lay(drawn.filter((copy) => copy.parent === null), null);
  // Every copy that is no whole, at its box's place plus its own in it.
  const positions = new Map<string, Point>();
  const put = (key: string, at: Point) => {
    const parts = inside.get(key);
    if (!parts) positions.set(key, at);
    else for (const [part, place] of parts) put(part, { x: at.x + place.x, y: at.y + place.y });
  };
  for (const [key, at] of top) put(key, at);
  return positions;
}

/** Each edge once, between two different Artifacts the graph has, by their numbers in
 * `graph.nodes`, and which Artifacts are shared inputs. */
function links(graph: Level) {
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

/** ELK's layout of `graph`, with the options the Graph has always given it and `room`; the
 * shared inputs and their edges are left out of it and stand in a column on its left. */
async function elk(graph: Level, room: Room): Promise<Map<string, Point>> {
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
      "elk.layered.spacing.nodeNodeBetweenLayers": String(room.x),
      "elk.spacing.nodeNode": String(room.y),
      "elk.spacing.componentComponent": String(room.apart),
      // Keep the tree's order (upstream first) within a layer where crossings allow. With the
      // shared inputs left out it costs little: 3-15% more than without it at 1,000 Artifacts.
      "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
    },
    children: graph.nodes.filter((node) => !inputs.has(node.id)).map((node) => ({ id: node.id, ...size(graph, node.id) })),
    edges: graph.edges
      .filter((edge) => !inputs.has(edge.source) && !inputs.has(edge.target))
      .map((edge, index) => ({ id: `e${index}`, sources: [edge.source], targets: [edge.target] })),
  });
  const [column, left] = stack([...inputs], graph, room);
  return new Map([...column, ...(laid.children ?? []).map((child): [string, Point] => [child.id, { x: left + (child.x ?? 0), y: child.y ?? 0 }])]);
}

/** The shared inputs `ids` of `graph` in one column, each under the one before, and how far
 * right of it the rest begins. */
function stack(ids: string[], graph: Level, room: Room): [[string, Point][], number] {
  let [y, wide] = [0, 0];
  const column = ids.map((id): [string, Point] => {
    const at = { x: 0, y };
    const { width, height } = size(graph, id);
    [y, wide] = [y + height + room.y, Math.max(wide, width)];
    return [id, at];
  });
  return [column, ids.length > 0 ? wide + room.x + room.apart : 0];
}

/** Top-left positions of `graph`'s nodes, layered and packed with `room` in time about linear
 * in its size. */
export function layered(graph: Level, room: Room = ROOM): Map<string, Point> {
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
  return pack(inputs, parts, graph, room);
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
function pack(inputs: number[], parts: Part[], graph: Level, room: Room): Map<string, Point> {
  const [column, left] = stack(inputs.map((node) => graph.nodes[node].id), graph, room);
  const placed = new Map<string, Point>(column);
  const cells = parts.map((part) => cell(part, graph, room));
  const corners = shelve(cells, room.apart);
  parts.forEach((part, each) => {
    const { x, y } = corners[each];
    part.members.forEach((node, at) => {
      placed.set(graph.nodes[node].id, { x: left + x + cells[each].x[at], y: y + cells[each].y[at] });
    });
  });
  return placed;
}

/** Where each Artifact of `part` sits in it, and its size: a column as wide as its widest
 * Artifact, `room.x` after the one before; in a column, each Artifact on its row's line, or
 * `room.y` below the one above if that is taller. Artifacts of `NODE`'s size sit on a grid. */
function cell(part: Part, graph: Level, room: Room): { x: Float64Array; y: Float64Array; width: number; height: number } {
  const sizes = part.members.map((node) => size(graph, graph.nodes[node].id));
  const wide = new Float64Array(part.columns);
  part.members.forEach((_, at) => (wide[part.column[at]] = Math.max(wide[part.column[at]], sizes[at].width)));
  const left = new Float64Array(part.columns);
  for (let at = 1; at < part.columns; at += 1) left[at] = left[at - 1] + wide[at - 1] + room.x;
  const x = Float64Array.from(part.members, (_, at) => left[part.column[at]]);
  const y = new Float64Array(part.members.length);
  const below = new Float64Array(part.columns).fill(-Infinity);
  const order = part.members.map((_, at) => at).sort((one, other) => part.row[one] - part.row[other]);
  let height = 0;
  for (const at of order) {
    const column = part.column[at];
    y[at] = Math.max(part.row[at] * (NODE.height + room.y), below[column] + room.y);
    below[column] = y[at] + sizes[at].height;
    height = Math.max(height, below[column]);
  }
  return { x, y, width: left[part.columns - 1] + wide[part.columns - 1], height };
}

/** Top-left corners of `boxes` packed in rows, in their order, `apart` from each other, into a
 * box about `ASPECT` wide. */
function shelve(boxes: { width: number; height: number }[], apart: number): Point[] {
  const area = boxes.reduce((sum, box) => sum + (box.width + apart) * (box.height + apart), 0);
  const width = Math.max(Math.sqrt(area * ASPECT), largest(boxes.map((box) => box.width)));
  let [x, y, tallest] = [0, 0, 0];
  return boxes.map((box) => {
    if (x > 0 && x + box.width > width) [x, y, tallest] = [0, y + tallest + apart, 0];
    const corner = { x, y };
    x += box.width + apart;
    tallest = Math.max(tallest, box.height);
    return corner;
  });
}
