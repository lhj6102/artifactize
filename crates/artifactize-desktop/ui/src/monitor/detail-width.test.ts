// The Detail's dragged width stays inside the graph's area and is kept for the next launch.
import { afterEach, expect, it } from "vitest";
import { KEEP, MIN, clampWidth, loadWidth, saveWidth } from "./detail-width";

afterEach(() => localStorage.clear());

it("fits a dragged width between a readable column and keeping some graph visible", () => {
  expect(clampWidth(500, 1200)).toBe(500);
  expect(clampWidth(100, 1200)).toBe(MIN);
  expect(clampWidth(5000, 1200)).toBe(1200 - KEEP);
  // An area too narrow for both keeps the Detail inside it.
  expect(clampWidth(500, 300)).toBe(300);
});

it("keeps the width for the next launch", () => {
  expect(loadWidth()).toBeNull();
  saveWidth(512);
  expect(loadWidth()).toBe(512);
});
