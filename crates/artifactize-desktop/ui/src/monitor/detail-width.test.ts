// The floating panels' dragged widths stay inside the graph's area, both panels and some graph
// between them fitting, and are kept for the next launch.
import { afterEach, expect, it } from "vitest";
import { DETAIL_KEY, GAP, KEEP, MIN, STAGE, TERMINAL_KEY, clampWidth, fitPanels, loadWidth, saveWidth } from "./detail-width";

afterEach(() => localStorage.clear());

it("fits a dragged width between a readable column and keeping some graph visible", () => {
  expect(clampWidth(500, 1200)).toBe(500);
  expect(clampWidth(100, 1200)).toBe(MIN);
  expect(clampWidth(5000, 1200)).toBe(1200 - GAP - KEEP);
  // An area too narrow for both keeps the Detail inside its gaps.
  expect(clampWidth(500, 300)).toBe(300 - 2 * GAP);
});

it("fits both panels with some graph between them, the terminal keeping room for a Detail", () => {
  expect(STAGE).toBe(760);
  // The narrowest stage holds both at their narrowest, whatever they want.
  expect(fitPanels(STAGE, 5000, 5000)).toEqual({ terminal: MIN, detail: MIN });
  // A wide terminal leaves a Detail its narrowest and the graph its strip.
  const wide = fitPanels(1100, 5000, null);
  expect(wide).toEqual({ terminal: 1100 - 2 * GAP - KEEP - MIN, detail: null });
  // The Detail takes what the terminal leaves; alone it reaches as far as before.
  const both = fitPanels(1100, 400, 5000);
  expect(both.terminal! + both.detail! + 2 * GAP + KEEP).toBe(1100);
  expect(fitPanels(1100, null, 5000).detail).toBe(1100 - GAP - KEEP);
  // Widths that fit stay as wanted.
  expect(fitPanels(1600, 480, 600)).toEqual({ terminal: 480, detail: 600 });
});

it("keeps each width for the next launch", () => {
  expect(loadWidth(DETAIL_KEY)).toBeNull();
  saveWidth(DETAIL_KEY, 512);
  saveWidth(TERMINAL_KEY, 440);
  expect(loadWidth(DETAIL_KEY)).toBe(512);
  expect(loadWidth(TERMINAL_KEY)).toBe(440);
});
