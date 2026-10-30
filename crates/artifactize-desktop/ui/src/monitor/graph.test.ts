// A known shape takes new lights by position, keeping its Artifacts and edges.
import { describe, expect, it } from "vitest";
import type { Shape } from "../ipc/client";
import { relight } from "./graph";

//   base ─┬─> app ──> site
//   lib  ─┘
const shape: Shape = {
  nodes: ["base", "lib", "app", "site"].map((name) => ({ id: `a:${name}`, name, light: "waiting", note: null, parts: [] })),
  edges: [
    { source: "a:base", target: "a:app", light: "waiting" },
    { source: "a:lib", target: "a:app", light: "waiting" },
    { source: "a:app", target: "a:site", light: "waiting" },
  ],
};

describe("relight", () => {
  it("lights each node and edge with the light at its position, and changes nothing else", () => {
    const lit = relight(shape, { kind: "lights", nodes: ["green", "red", "blocked", "waiting"], edges: ["green", "red", "blocked"] });
    expect(lit.nodes.map((node) => [node.id, node.light])).toEqual([["a:base", "green"], ["a:lib", "red"], ["a:app", "blocked"], ["a:site", "waiting"]]);
    expect(lit.edges.map((edge) => [edge.source, edge.target, edge.light])).toEqual([
      ["a:base", "a:app", "green"],
      ["a:lib", "a:app", "red"],
      ["a:app", "a:site", "blocked"],
    ]);
    expect(shape.nodes.every((node) => node.light === "waiting")).toBe(true);
  });

  it("keeps the shape itself when no light changed", () => {
    expect(relight(shape, { kind: "lights", nodes: shape.nodes.map((node) => node.light), edges: shape.edges.map((edge) => edge.light) })).toBe(shape);
  });
});
