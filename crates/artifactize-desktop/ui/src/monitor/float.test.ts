// Where a floating box goes beside its anchor: right, else left, else below, else above, and
// always inside the area it floats in.
import { describe, expect, it } from "vitest";
import { place } from "./float";

const area = { left: 100, top: 50, right: 900, bottom: 650 };
const box = (left: number, top: number, width: number, height: number) => ({ left, top, right: left + width, bottom: top + height });

describe("place", () => {
  it("puts the box to the right of its anchor, top-aligned, in the area's coordinates", () => {
    expect(place(box(200, 100, 168, 44), area, 300, 200)).toEqual({ x: 276, y: 50 });
  });

  it("puts it to the left when the right has no room", () => {
    expect(place(box(700, 100, 168, 44), area, 300, 200)).toEqual({ x: 292, y: 50 });
  });

  it("puts it below an anchor as wide as the area, and above when below has no room", () => {
    expect(place(box(110, 100, 780, 24), area, 300, 200)).toEqual({ x: 10, y: 82 });
    expect(place(box(110, 500, 780, 24), area, 300, 200)).toEqual({ x: 10, y: 242 });
  });

  it("keeps the box inside the area beside an anchor near its bottom", () => {
    expect(place(box(200, 620, 168, 44), area, 300, 200)).toEqual({ x: 276, y: 392 });
  });
});
