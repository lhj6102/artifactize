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
import { boxes, type SpaceBox } from "./spaces";

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
  nodes: names.map((name) => ({ id: `a:${name}`, name, space: null, label: name, light: "waiting", note: null })),
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

describe("spaces", () => {
  /** `graph` with each Artifact named `space::name` in that space. */
  const spaced = (graph: Shape): Shape => ({
    ...graph,
    nodes: graph.nodes.map((node) => {
      const [space, label] = node.name.includes("::") ? node.name.split("::") : [null, node.name];
      return { ...node, space, label };
    }),
  });
  const inside = (at: Point, box: SpaceBox) =>
    at.x >= box.x && at.y >= box.y && at.x + NODE.width <= box.x + box.width && at.y + NODE.height <= box.y + box.height;
  const meets = (one: SpaceBox, other: { x: number; y: number; width: number; height: number }) =>
    one.x < other.x + other.width && other.x < one.x + one.width && one.y < other.y + other.height && other.y < one.y + one.height;

  /** Each space's box holds its own Artifacts and no other, and no two boxes meet. */
  function grouped(graph: Shape, placed: Map<string, Point>): SpaceBox[] {
    const drawn = boxes(graph, placed);
    for (const box of drawn) {
      for (const node of graph.nodes) {
        const at = placed.get(node.id)!;
        if (node.space === box.space) expect(inside(at, box), `${node.name} in ${box.space}`).toBe(true);
        else expect(meets(box, { ...at, ...NODE }), `${node.name} outside ${box.space}`).toBe(false);
      }
      for (const other of drawn) if (other !== box) expect(meets(box, other), `${box.space} apart from ${other.space}`).toBe(false);
    }
    return drawn;
  }

  it("lays out each space's Artifacts together in its box, a space right of the spaces it uses", async () => {
    //   base ─> combat::Spec ─> combat::Code ─> docs
    //                └──────> inventory::Code ─┘        lone
    const graph = spaced(
      shape(
        ["base", "combat::Spec", "combat::Code", "inventory::Code", "docs", "lone"],
        [
          ["base", "combat::Spec"],
          ["combat::Spec", "combat::Code"],
          ["combat::Spec", "inventory::Code"],
          ["combat::Code", "docs"],
          ["inventory::Code", "docs"],
        ],
      ),
    );
    const placed = await layout(graph);
    expect(placed.size).toBe(graph.nodes.length);
    expect(overlaps(placed)).toEqual([]);
    const [combat, inventory] = grouped(graph, placed);
    expect([combat.space, inventory.space]).toEqual(["combat", "inventory"]);
    expect(inventory.x).toBeGreaterThanOrEqual(combat.x + combat.width);
    expect(backward(graph, placed)).toEqual([]);
  });

  it("lays out a whole catalog with a space per skill, its spaces packed rather than one tall column", async () => {
    const whole = catalog();
    const graph: Shape = {
      ...whole,
      nodes: whole.nodes.map((node) => {
        const skill = /^[tc](\d+)-/.exec(node.name)?.[1];
        return skill === undefined ? node : { ...node, name: `s${skill}::${node.name}`, space: `s${skill}`, label: node.name };
      }),
    };
    const placed = await layout(graph);
    expect(placed.size).toBe(graph.nodes.length);
    expect(overlaps(placed)).toEqual([]);
    expect(aspect(placed)).toBeLessThan(4);
    expect(aspect(placed)).toBeGreaterThan(0.25);
    // Every box against every other is slow at a catalog's size: a sample of them is checked.
    const drawn = boxes(graph, placed);
    expect(drawn.length).toBeGreaterThan(500);
    for (const box of drawn.filter((_, at) => at % 50 === 0)) {
      for (const other of drawn) if (other !== box) expect(meets(box, other)).toBe(false);
    }
  });
});
