// The stage keeps its narrowest in pixels beside the Pane, whatever layout was kept or dragged.
import { expect, it } from "vitest";
import { fitLayout } from "./columns";
import { STAGE } from "./detail-width";

it("gives the stage its narrowest back from a Pane kept or dragged too wide", () => {
  // At the narrowest window, 1280 less the Git layer: a Pane at half leaves too little.
  const fitted = fitLayout([50, 50], 1048)!;
  expect(Math.round((fitted[1] / 100) * 1048)).toBe(STAGE + 1);
  expect(fitted[0] + fitted[1]).toBeCloseTo(100);
  // A layout that fits stays, as does one before the columns have a width.
  expect(fitLayout([24, 76], 1048)).toBeNull();
  expect(fitLayout([50, 50], 0)).toBeNull();
});
