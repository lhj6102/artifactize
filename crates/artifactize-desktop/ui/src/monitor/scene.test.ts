// A laid-out catalog as the canvas finds things in it: what a camera shows, the Artifacts in a
// part of it, which it stops counting past a limit, and the Artifact under the pointer.
import { describe, expect, it } from "vitest";
import { NODE, layered } from "./layout";
import { SHARED, catalog } from "./catalog.fixture";
import { hit, scene, view, within } from "./scene";

const graph = catalog();
const laid = scene(layered(graph));

describe("scene", () => {
  it("bounds every Artifact", () => {
    expect(laid.ids).toHaveLength(graph.nodes.length);
    expect(laid.places.every((at) => at.x >= laid.bounds.x && at.y >= laid.bounds.y
      && at.x + NODE.width <= laid.bounds.x + laid.bounds.width && at.y + NODE.height <= laid.bounds.y + laid.bounds.height)).toBe(true);
  });

  it("finds the Artifacts in a part of the graph in layout order, and stops counting past a limit", () => {
    const whole = within(laid, laid.bounds);
    expect(whole).toHaveLength(graph.nodes.length);
    expect(within(laid, laid.bounds, 500)).toBeNull();
    // A box around the shared input holds it and none beside it.
    const at = laid.places[laid.numbers.get(SHARED)!];
    expect(within(laid, { x: at.x + 1, y: at.y + 1, width: 10, height: 10 }, 500)?.map((number) => laid.ids[number])).toEqual([SHARED]);
    const some = within(laid, { x: laid.bounds.x, y: laid.bounds.y, width: 2000, height: 1000 }, 500)!;
    expect(some.length).toBeGreaterThan(0);
    expect(some).toEqual([...some].sort((one, other) => one - other));
  });

  it("finds the Artifact under a point, and none between them", () => {
    const at = laid.places[laid.numbers.get(SHARED)!];
    expect(hit(laid, { x: at.x + NODE.width / 2, y: at.y + NODE.height / 2 })).toBe(SHARED);
    expect(hit(laid, { x: at.x + NODE.width + 1, y: at.y + NODE.height / 2 })).toBeNull();
  });

  it("shows under a camera what its canvas covers, with a margin", () => {
    const shown = view({ x: -100, y: -50, zoom: 0.5 }, 800, 600);
    // The canvas covers 1600 × 1200 of the graph from (200, 100); a quarter more on each side.
    expect(shown).toEqual({ x: 200 - 400, y: 100 - 300, width: 2400, height: 1800 });
  });
});
