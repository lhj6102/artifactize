// A branch's chip row: chips oldest to newest with the shown one filled, a click chooses a chip,
// a drag of the row chooses nothing, and the oldest end in view asks for older Runs once.
import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import ChipRow from "./ChipRow.svelte";
import { branch, runChip } from "./fixtures";

beforeAll(() => {
  Element.prototype.scrollIntoView ??= () => {};
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(cleanup);

const row = () => screen.getByRole("group", { name: "Runs of app main" });

it("shows the chips oldest first with the shown one filled, and a click chooses one", async () => {
  const onchoose = vi.fn();
  const chips = [runChip("run-1", "RED"), runChip("run-2", "GREEN"), { kind: "current" as const }];
  render(ChipRow, { branch: branch("main", chips), filled: 1, onchoose, onolder: vi.fn() });
  const buttons = within(row()).getAllByRole("button");
  expect(buttons.map((button) => button.textContent?.trim())).toEqual(["✗RED 5m", "✓GREEN 5m", "Current"]);
  expect(buttons.map((button) => button.getAttribute("aria-pressed"))).toEqual(["false", "true", "false"]);
  await fireEvent.click(buttons[2]);
  expect(onchoose).toHaveBeenCalledWith({ kind: "current" });
});

it("drags sideways while the mouse is held, and the click that ends a drag chooses nothing", async () => {
  const onchoose = vi.fn();
  render(ChipRow, { branch: branch("main", [runChip("run-1", "RED"), runChip("run-2", "GREEN")]), filled: null, onchoose, onolder: vi.fn() });
  const first = within(row()).getAllByRole("button")[0];
  await fireEvent.mouseDown(first, { button: 0, clientX: 100 });
  await fireEvent.mouseMove(window, { clientX: 60 });
  await fireEvent.mouseUp(window, { clientX: 60 });
  await fireEvent.click(first);
  expect(onchoose).not.toHaveBeenCalled();
  // A press without a drag is a click.
  await fireEvent.mouseDown(first, { button: 0, clientX: 100 });
  await fireEvent.mouseUp(window, { clientX: 101 });
  await fireEvent.click(first);
  expect(onchoose).toHaveBeenCalledOnce();
});

it("asks for older Runs when its oldest end is in view and older ones exist, and only then", () => {
  const onolder = vi.fn();
  render(ChipRow, { branch: branch("main", [runChip("run-1", "RED")], true), filled: null, onchoose: vi.fn(), onolder });
  expect(onolder).toHaveBeenCalledOnce();
  cleanup();
  const none = vi.fn();
  render(ChipRow, { branch: branch("main", [runChip("run-1", "RED")]), filled: null, onchoose: vi.fn(), onolder: none });
  expect(none).not.toHaveBeenCalled();
});
