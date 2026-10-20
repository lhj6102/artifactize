// The layout places every Artifact once, without overlaps, each to the right of the Artifacts
// it uses, and keeps a whole catalog (a shared input used by every review included) in a box
// shaped like a screen rather than one long column.
import { describe, expect, it } from "vitest";
import type { Shape } from "../ipc/client";
import type { Point } from "./graph";
import { NODE, layout } from "./layout";
import { REVIEWS, SHARED, catalog } from "./catalog.fixture";

const shape = (names: string[], pairs: [string, string][]): Shape => ({
  nodes: names.map((name) => ({ id: `a:${name}`, name, light: "waiting" })),
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

describe("layout", () => {
  it("places each Artifact right of the ones it uses, in layers that do not overlap", () => {
    //   base ─┬─> app ──> site
    //   lib  ─┘      └──> docs      lone
    const graph = shape(["base", "lib", "app", "site", "docs", "lone"], [["base", "app"], ["lib", "app"], ["app", "site"], ["app", "docs"]]);
    const placed = layout(graph);
    expect([...placed.keys()].sort()).toEqual(graph.nodes.map((node) => node.id).sort());
    expect(backward(graph, placed)).toEqual([]);
    expect(placed.get("a:base")!.x).toBe(placed.get("a:lib")!.x);
    expect(placed.get("a:site")!.x).toBe(placed.get("a:docs")!.x);
    expect(overlaps(placed)).toEqual([]);
  });

  it("lays out a cycle, its edge that closes it the only one pointing back", () => {
    const graph = shape(["one", "two", "three"], [["one", "two"], ["two", "three"], ["three", "one"]]);
    const placed = layout(graph);
    expect(placed.size).toBe(3);
    expect(backward(graph, placed).map((edge) => `${edge.source}>${edge.target}`)).toEqual(["a:three>a:one"]);
    expect(overlaps(placed)).toEqual([]);
  });

  it("sets a shared input apart in its own column instead of stacking every part that uses it", () => {
    // 100 two-Artifact parts, each using `criteria`.
    const parts = Array.from({ length: 100 }, (_, at) => [`tip${at}`, `chain${at}`]);
    const graph = shape(["criteria", ...parts.flat()], parts.flatMap(([tip, chain]): [string, string][] => [[tip, chain], ["criteria", chain]]));
    const placed = layout(graph);
    expect(backward(graph, placed)).toEqual([]);
    expect(overlaps(placed)).toEqual([]);
    // Every chain is right of the shared input, which no part's Artifact shares a column with.
    const criteria = placed.get("a:criteria")!;
    expect(parts.every(([, chain]) => placed.get(`a:${chain}`)!.x > criteria.x)).toBe(true);
    expect(parts.flat().some((name) => placed.get(`a:${name}`)!.x === criteria.x)).toBe(false);
    // The parts pack into rows: the drawing is wider than tall, not one column of 100.
    expect(aspect(placed)).toBeGreaterThan(1);
  });

  it("lays out a whole catalog, its shared input included, without overlaps and about as wide as a screen", () => {
    const graph = catalog();
    expect(graph.nodes.length).toBeGreaterThan(21000);
    expect(graph.edges.length).toBeGreaterThan(48000);
    expect(graph.edges.filter((edge) => edge.source === SHARED)).toHaveLength(REVIEWS);
    // The test's own time limit bounds the layout: one that grew with the square of the graph,
    // or that tied every part to the shared input, would not finish within it.
    const placed = layout(graph);
    expect(placed.size).toBe(graph.nodes.length);
    expect(backward(graph, placed)).toEqual([]);
    expect(overlaps(placed)).toEqual([]);
    expect(aspect(placed)).toBeGreaterThan(0.5);
    expect(aspect(placed)).toBeLessThan(4);
  });
});
