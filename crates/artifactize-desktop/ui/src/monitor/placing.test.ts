// Where a drop on a Current's graph goes and which places it saves: the seam between a drag
// and the edit the library is asked for.
import { describe, expect, it } from "vitest";
import type { Shape } from "../ipc/client";
import { NO_ANNOTATIONS } from "./annotations";
import { BOX, boxes, copies } from "./parts";
import { dropAt, placesOf } from "./placing";

//   app ⊃ { kit }, bundle ⊃ { kit }; core, docs at the top level
const graph: Shape = {
  nodes: ["app", "bundle", "kit", "core", "docs"].map((name) => ({
    id: `a:${name}`,
    name,
    light: "waiting",
    tally: null,
    note: null,
    parts: name === "app" || name === "bundle" ? ["a:kit"] : [],
  })),
  edges: [],
};
const drawn = copies(graph);
const positions = new Map([
  ["a:app/a:kit", { x: BOX.pad, y: BOX.header }],
  ["a:bundle/a:kit", { x: 400 + BOX.pad, y: BOX.header }],
  ["a:core", { x: 0, y: 300 }],
  ["a:docs", { x: 400, y: 300 }],
]);
const rects = boxes(drawn, positions);
const copy = (key: string) => drawn.find((found) => found.key === key)!;

describe("a drop", () => {
  it("over a whole's box, outside its parts, makes the dragged Artifact its part", () => {
    const drop = dropAt(drawn, rects, copy("a:core"), { x: 10, y: 10 });
    expect(drop).toEqual({ kind: "join", to: "app", key: "a:app" });
  });

  it("onto a node makes that node its whole", () => {
    expect(dropAt(drawn, rects, copy("a:core"), { x: 410, y: 310 })).toEqual({ kind: "join", to: "docs", key: "a:docs" });
  });

  it("onto an Artifact inside the dragged one is still that Artifact, which the library then refuses", () => {
    const drop = dropAt(drawn, rects, copy("a:app"), { x: 400 + BOX.pad + 10, y: BOX.header + 10 });
    expect(drop).toEqual({ kind: "join", to: "kit", key: "a:bundle/a:kit" });
  });

  it("onto a whole that already holds the dragged Artifact is that drop, for the library to refuse, never a leave in its place", () => {
    // kit, dragged out of app, over bundle's box (bundle already holds kit) and over bundle's kit.
    expect(dropAt(drawn, rects, copy("a:app/a:kit"), { x: 410, y: 10 })).toEqual({ kind: "join", to: "bundle", key: "a:bundle" });
    expect(dropAt(drawn, rects, copy("a:app/a:kit"), { x: 400 + BOX.pad + 10, y: BOX.header + 10 })).toEqual({ kind: "join", to: "kit", key: "a:bundle/a:kit" });
  });

  it("outside every box takes a part out of its whole, and leaves a top-level Artifact placed", () => {
    expect(dropAt(drawn, rects, copy("a:app/a:kit"), { x: 900, y: 900 })).toEqual({ kind: "leave" });
    expect(dropAt(drawn, rects, copy("a:core"), { x: 900, y: 900 })).toEqual({ kind: "place" });
  });

  it("inside its own box places a part there", () => {
    expect(dropAt(drawn, rects, copy("a:app/a:kit"), { x: 10, y: 10 })).toEqual({ kind: "place" });
  });
});

describe("the places a move saves", () => {
  it("are every top-level node's while the top level is laid out automatically, and the moved one's after", () => {
    const first = placesOf(drawn, rects, NO_ANNOTATIONS, copy("a:core"), { x: 20, y: 500 });
    expect(Object.keys(first).sort()).toEqual(["app", "bundle", "core", "docs"]);
    expect(first.core).toEqual([20, 500]);
    const placed = { ...NO_ANNOTATIONS, at: new Map([["core", { x: 20, y: 500 }]]) };
    expect(placesOf(drawn, rects, placed, copy("a:core"), { x: 60, y: 500 })).toEqual({ core: [60, 500] });
  });

  it("are a box's parts' places from its inner corner, in its whole's file alone", () => {
    const box = rects.get("a:app")!;
    const placed = placesOf(drawn, rects, NO_ANNOTATIONS, copy("a:app/a:kit"), { x: box.x + BOX.pad + 30, y: box.y + BOX.header + 12 });
    expect(placed).toEqual({ "app/kit": [30, 12] });
  });
});
