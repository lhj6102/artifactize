// A graph of at most ELK_LIMIT Artifacts is laid out by ELK with the Graph's options, its shared
// inputs set apart; a larger one by the layered layout, which places every Artifact once,
// without overlaps, each to the right of the Artifacts it uses, and keeps a whole catalog (a
// shared input used by every review included) in a box shaped like a screen rather than one
// long column. ELK runs here in elkjs's in-thread build instead of its Web Worker.
import ELK, { type ElkNode } from "elkjs/lib/elk.bundled.js";
import { describe, expect, it, vi } from "vitest";
import type { Shape } from "../ipc/client";
import type { Point } from "./graph";
import { ELK_LIMIT, NODE, layered, layout } from "./layout";
import { REVIEWS, SHARED, catalog } from "./catalog.fixture";
import { boxes, copies, type Rect } from "./parts";

/** How often the app loaded ELK, started it, and asked it for a layout. */
const elk = vi.hoisted(() => ({ loads: 0, starts: 0, layouts: 0 }));
vi.mock("elkjs/lib/elk-api.js", async () => {
  elk.loads += 1;
  const { default: Bundled } = await import("elkjs/lib/elk.bundled.js");
  return {
    default: class {
      readonly #elk = new Bundled();
      constructor() {
        elk.starts += 1;
      }
      layout(graph: ElkNode) {
        elk.layouts += 1;
        return this.#elk.layout(graph);
      }
    },
  };
});

/** ELK's own layout of `graph` with the options the Graph gave it before #320. */
async function byElk(graph: Shape): Promise<Map<string, Point>> {
  const laid = await new ELK().layout({
    id: "run",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.layered.spacing.nodeNodeBetweenLayers": "72",
      "elk.spacing.nodeNode": "20",
      "elk.spacing.componentComponent": "40",
      "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
    },
    children: graph.nodes.map((node) => ({ id: node.id, width: 168, height: 44 })),
    edges: graph.edges.map((edge, index) => ({ id: `e${index}`, sources: [edge.source], targets: [edge.target] })),
  });
  return new Map((laid.children ?? []).map((child) => [child.id, { x: child.x ?? 0, y: child.y ?? 0 }]));
}

const shape = (names: string[], pairs: [string, string][]): Shape => ({
  nodes: names.map((name) => ({ id: `a:${name}`, name, light: "waiting", note: null, parts: [] })),
  edges: pairs.map(([source, target]) => ({ source: `a:${source}`, target: `a:${target}`, light: "waiting" })),
});

/** Pairs of Artifacts whose boxes overlap, found cell by cell. */
function overlaps(placed: Map<string, Point>): [string, string][] {
  const cells = new Map<string, [string, Point][]>();
  const found: [string, string][] = [];
  for (const [id, at] of placed) {
    const key = `${Math.floor(at.x / NODE.width)},${Math.floor(at.y / NODE.height)}`;
    for (const dx of [-1, 0, 1]) {
      for (const dy of [-1, 0, 1]) {
        const near = cells.get(`${Math.floor(at.x / NODE.width) + dx},${Math.floor(at.y / NODE.height) + dy}`) ?? [];
        for (const [other, there] of near) {
          if (Math.abs(there.x - at.x) < NODE.width && Math.abs(there.y - at.y) < NODE.height) found.push([other, id]);
        }
      }
    }
    cells.set(key, [...(cells.get(key) ?? []), [id, at]]);
  }
  return found;
}

/** The edges whose target is not to the right of their source. */
const backward = (graph: Shape, placed: Map<string, Point>) =>
  graph.edges.filter((edge) => placed.get(edge.source)!.x + NODE.width > placed.get(edge.target)!.x);

/** The box every Artifact fills together: width over height. */
function aspect(placed: Map<string, Point>): number {
  const xs = [...placed.values()].map((at) => at.x);
  const ys = [...placed.values()].map((at) => at.y);
  const width = xs.reduce((top, x) => Math.max(top, x), 0) + NODE.width - xs.reduce((low, x) => Math.min(low, x), Infinity);
  const height = ys.reduce((top, y) => Math.max(top, y), 0) + NODE.height - ys.reduce((low, y) => Math.min(low, y), Infinity);
  return width / height;
}

/** `count` Artifacts in chains of four. */
function chains(count: number): Shape {
  const names = Array.from({ length: count }, (_, at) => `n${at}`);
  return shape(names, names.flatMap((name, at): [string, string][] => (at % 4 === 0 ? [] : [[names[at - 1], name]])));
}

/** 100 two-Artifact parts, each using `criteria`. */
const parts = Array.from({ length: 100 }, (_, at) => [`tip${at}`, `chain${at}`]);
const criteria = shape(["criteria", ...parts.flat()], parts.flatMap(([tip, chain]): [string, string][] => [[tip, chain], ["criteria", chain]]));

describe("layout", () => {
  it("lays out a graph over ELK_LIMIT with the layered layout, without ever loading ELK", async () => {
    vi.resetModules();
    const fresh = await import("./layout");
    const graph = chains(ELK_LIMIT + 1);
    expect(await fresh.layout(graph)).toEqual(layered(graph));
    expect(elk).toEqual({ loads: 0, starts: 0, layouts: 0 });
    // ELK starts on the first graph it lays out.
    await fresh.layout(chains(3));
    expect(elk).toEqual({ loads: 1, starts: 1, layouts: 1 });
  });

  it("lays out a small graph exactly as ELK does with the Graph's options", async () => {
    //   base ─┬─> app ──> site
    //   lib  ─┘      └──> docs      lone
    const graph = shape(["base", "lib", "app", "site", "docs", "lone"], [["base", "app"], ["lib", "app"], ["app", "site"], ["app", "docs"]]);
    expect(await layout(graph)).toEqual(await byElk(graph));
  });

  it("switches from ELK to the layered layout just past ELK_LIMIT", async () => {
    const before = elk.layouts;
    const most = chains(ELK_LIMIT);
    expect(await layout(most)).toEqual(await byElk(most));
    expect(elk.layouts).toBe(before + 1);
    const over = chains(ELK_LIMIT + 1);
    expect(await layout(over)).toEqual(layered(over));
    expect(elk.layouts).toBe(before + 1);
  });

  it("sets a shared input apart for ELK, which lays out the rest as without it", async () => {
    const placed = await layout(criteria);
    const rest = await byElk(shape(parts.flat(), parts.map(([tip, chain]): [string, string] => [tip, chain])));
    const at = placed.get("a:criteria")!;
    // The rest keeps ELK's own layout, moved right of the shared input's column as a whole.
    const shift = placed.get("a:tip0")!.x - rest.get("a:tip0")!.x;
    expect(shift).toBeGreaterThanOrEqual(at.x + NODE.width);
    expect(new Map([...placed].filter(([id]) => id !== "a:criteria"))).toEqual(
      new Map([...rest].map(([id, point]) => [id, { x: point.x + shift, y: point.y }])),
    );
    expect(overlaps(placed)).toEqual([]);
  });
});

describe("layered", () => {
  it("places each Artifact right of the ones it uses, in layers that do not overlap", () => {
    //   base ─┬─> app ──> site
    //   lib  ─┘      └──> docs      lone
    const graph = shape(["base", "lib", "app", "site", "docs", "lone"], [["base", "app"], ["lib", "app"], ["app", "site"], ["app", "docs"]]);
    const placed = layered(graph);
    expect([...placed.keys()].sort()).toEqual(graph.nodes.map((node) => node.id).sort());
    expect(backward(graph, placed)).toEqual([]);
    expect(placed.get("a:base")!.x).toBe(placed.get("a:lib")!.x);
    expect(placed.get("a:site")!.x).toBe(placed.get("a:docs")!.x);
    expect(overlaps(placed)).toEqual([]);
  });

  it("lays out a cycle, its edge that closes it the only one pointing back", () => {
    const graph = shape(["one", "two", "three"], [["one", "two"], ["two", "three"], ["three", "one"]]);
    const placed = layered(graph);
    expect(placed.size).toBe(3);
    expect(backward(graph, placed).map((edge) => `${edge.source}>${edge.target}`)).toEqual(["a:three>a:one"]);
    expect(overlaps(placed)).toEqual([]);
  });

  it("sets a shared input apart in its own column instead of stacking every part that uses it", () => {
    const placed = layered(criteria);
    expect(backward(criteria, placed)).toEqual([]);
    expect(overlaps(placed)).toEqual([]);
    // Every chain is right of the shared input, which no part's Artifact shares a column with.
    const at = placed.get("a:criteria")!;
    expect(parts.every(([, chain]) => placed.get(`a:${chain}`)!.x > at.x)).toBe(true);
    expect(parts.flat().some((name) => placed.get(`a:${name}`)!.x === at.x)).toBe(false);
    // The parts pack into rows: the drawing is wider than tall, not one column of 100.
    expect(aspect(placed)).toBeGreaterThan(1);
  });

  it("lays out a whole catalog, its shared input included, without overlaps and about as wide as a screen", async () => {
    const graph = catalog();
    expect(graph.nodes.length).toBeGreaterThan(21000);
    expect(graph.edges.length).toBeGreaterThan(48000);
    expect(graph.edges.filter((edge) => edge.source === SHARED)).toHaveLength(REVIEWS);
    // The test's own time limit bounds the layout: one that grew with the square of the graph,
    // or that tied every part to the shared input, would not finish within it.
    const before = elk.layouts;
    const placed = await layout(graph);
    expect(elk.layouts).toBe(before);
    expect(placed.size).toBe(graph.nodes.length);
    expect(backward(graph, placed)).toEqual([]);
    expect(overlaps(placed)).toEqual([]);
    expect(aspect(placed)).toBeGreaterThan(0.5);
    expect(aspect(placed)).toBeLessThan(4);
  });
});


/** `graph` with the wholes `parts` names, each with its parts. */
const made = (graph: Shape, parts: Record<string, string[]>): Shape => ({
  ...graph,
  nodes: graph.nodes.map((node) => ({ ...node, parts: (parts[node.name] ?? []).map((part) => `a:${part}`) })),
});

/** Every copy's box: each inside the box around it, and none overlapping another drawn beside
 * it in the same box (or in none). */
function nested(graph: Shape, placed: Map<string, Point>) {
  const drawn = copies(graph);
  const rects = boxes(drawn, placed);
  expect([...rects.keys()]).toEqual(drawn.map((copy) => copy.key));
  const inside = (inner: Rect, outer: Rect) =>
    inner.x >= outer.x && inner.y >= outer.y && inner.x + inner.width <= outer.x + outer.width && inner.y + inner.height <= outer.y + outer.height;
  const apart = (one: Rect, other: Rect) =>
    one.x + one.width <= other.x || other.x + other.width <= one.x || one.y + one.height <= other.y || other.y + other.height <= one.y;
  const levels = new Map<string | null, Rect[]>();
  for (const copy of drawn) {
    const at = rects.get(copy.key)!;
    if (copy.parent !== null) expect(inside(at, rects.get(copy.parent)!)).toBe(true);
    levels.set(copy.parent, [...(levels.get(copy.parent) ?? []), at]);
  }
  for (const level of levels.values()) {
    level.forEach((one, at) => level.slice(at + 1).forEach((other) => expect(apart(one, other)).toBe(true)));
  }
  return { drawn, rects };
}

describe("layout of wholes", () => {
  //   suite ⊃ { app ⊃ { kit, core }, site ⊃ { kit } }, docs ⊃ { site }; core ──> app's kit ──> lone
  const graph = made(
    shape(["suite", "docs", "app", "site", "kit", "core", "lone"], [["core", "kit"], ["kit", "lone"]]),
    { suite: ["app", "site"], docs: ["site"], app: ["kit", "core"], site: ["kit"] },
  );

  it("draws each whole around its parts, nested, and a shared part inside each of its wholes", async () => {
    const placed = await layout(graph);
    const { drawn } = nested(graph, placed);
    // Only copies that are no wholes are placed: kit three times, core and lone once each.
    expect([...placed.keys()].sort()).toEqual(
      ["a:docs/a:site/a:kit", "a:lone", "a:suite/a:app/a:core", "a:suite/a:app/a:kit", "a:suite/a:site/a:kit"].sort(),
    );
    expect(drawn.filter((copy) => copy.node.id === "a:site").map((copy) => copy.parent)).toEqual(["a:suite", "a:docs"]);
    // Within app its parts keep their edge left to right.
    expect(placed.get("a:suite/a:app/a:core")!.x).toBeLessThan(placed.get("a:suite/a:app/a:kit")!.x);
  });

  it("nests the same way past ELK_LIMIT with the layered layout", async () => {
    const before = elk.layouts;
    // 300 wholes of four parts each, in chains, and a whole of all the wholes.
    const names = Array.from({ length: 300 }, (_, at) => `w${at}`);
    const members = (whole: string) => [0, 1, 2, 3].map((at) => `${whole}p${at}`);
    const big = made(
      shape(["all", ...names, ...names.flatMap(members)], names.flatMap((whole) => members(whole).slice(1).map((part, at): [string, string] => [members(whole)[at], part]))),
      { all: names, ...Object.fromEntries(names.map((whole) => [whole, members(whole)])) },
    );
    const placed = await layout(big);
    expect(elk.layouts).toBe(before);
    expect(placed.size).toBe(1200);
    nested(big, placed);
  });
});
