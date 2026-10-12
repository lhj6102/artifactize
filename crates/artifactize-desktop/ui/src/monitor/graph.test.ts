// The Run graph's frontend rules: a layout depends only on Artifacts and edges, and arrow keys
// move between connected Artifacts.
import { describe, expect, it } from "vitest";
import type { RunGraph } from "../ipc/client";
import { neighbor, shape, type Point } from "./graph";

//   base ─┬─> app ──> site
//   lib  ─┘      └──> docs
const graph: RunGraph = {
  run: "run-a",
  nodes: ["base", "lib", "app", "site", "docs"].map((name) => ({ id: `a:${name}`, name, light: "waiting" })),
  edges: [
    { source: "a:base", target: "a:app", light: "green", dependency: false },
    { source: "a:lib", target: "a:app", light: "active", dependency: true },
    { source: "a:app", target: "a:site", light: "waiting", dependency: false },
    { source: "a:app", target: "a:docs", light: "waiting", dependency: false },
  ],
};

const at = new Map<string, Point>([
  ["a:base", { x: 0, y: 0 }],
  ["a:lib", { x: 0, y: 100 }],
  ["a:app", { x: 240, y: 50 }],
  ["a:site", { x: 480, y: 0 }],
  ["a:docs", { x: 480, y: 100 }],
]);

describe("shape", () => {
  it("ignores lights and order, never Artifacts or edges", () => {
    const relit: RunGraph = {
      ...graph,
      nodes: [...graph.nodes].reverse().map((node) => ({ ...node, light: "green" })),
      edges: graph.edges.map((edge) => ({ ...edge, light: "red" })),
    };
    expect(shape(relit)).toBe(shape(graph));
    expect(shape({ ...graph, edges: graph.edges.slice(1) })).not.toBe(shape(graph));
    expect(shape({ ...graph, nodes: graph.nodes.slice(1) })).not.toBe(shape(graph));
  });
});

describe("neighbor", () => {
  it("→ and ← follow edges to the nearest downstream or upstream Artifact", () => {
    expect(neighbor(graph, at, "a:base", "ArrowRight")).toBe("a:app");
    expect(neighbor(graph, at, "a:app", "ArrowRight")).toBe("a:site");
    expect(neighbor(graph, at, "a:app", "ArrowLeft")).toBe("a:base");
    expect(neighbor(graph, at, "a:site", "ArrowLeft")).toBe("a:app");
    expect(neighbor(graph, at, "a:site", "ArrowRight")).toBeNull();
  });

  it("↑ and ↓ move within a column between Artifacts that share a neighbour", () => {
    expect(neighbor(graph, at, "a:base", "ArrowDown")).toBe("a:lib");
    expect(neighbor(graph, at, "a:lib", "ArrowUp")).toBe("a:base");
    expect(neighbor(graph, at, "a:site", "ArrowDown")).toBe("a:docs");
    expect(neighbor(graph, at, "a:docs", "ArrowDown")).toBeNull();
    expect(neighbor(graph, at, "a:app", "ArrowUp")).toBeNull();
  });
});
