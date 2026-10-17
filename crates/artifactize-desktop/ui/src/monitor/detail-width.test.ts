// The Detail's dragged width stays inside the graph's area and is kept for the next launch.
import { afterEach, expect, it } from "vitest";
import { GAP, KEEP, MIN, clampWidth, loadWidth, saveWidth } from "./detail-width";

afterEach(() => localStorage.clear());

it("fits a dragged width between a readable column and keeping some graph visible", () => {
  expect(clampWidth(500, 1200)).toBe(500);
  expect(clampWidth(100, 1200)).toBe(MIN);
  expect(clampWidth(5000, 1200)).toBe(1200 - GAP - KEEP);
  // An area too narrow for both keeps the Detail inside its gaps.
  expect(clampWidth(500, 300)).toBe(300 - 2 * GAP);
});

it("keeps the width for the next launch", () => {
  expect(loadWidth()).toBeNull();
  saveWidth(512);
  expect(loadWidth()).toBe(512);
});
