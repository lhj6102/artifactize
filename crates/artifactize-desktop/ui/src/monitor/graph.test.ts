// The Run graph's frontend rule: a layout depends only on Artifacts and edges.
import { describe, expect, it } from "vitest";
import type { RunGraph } from "../ipc/client";
import { shape } from "./graph";

//   base ─┬─> app ──> site
//   lib  ─┘      └──> docs
const graph: RunGraph = {
  run: "run-a",
  nodes: ["base", "lib", "app", "site", "docs"].map((name) => ({ id: `a:${name}`, name, light: "waiting" })),
  edges: [
    { source: "a:base", target: "a:app", light: "green" },
    { source: "a:lib", target: "a:app", light: "active" },
    { source: "a:app", target: "a:site", light: "waiting" },
    { source: "a:app", target: "a:docs", light: "waiting" },
  ],
};

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
