// A branch's chip row: the Current chip, then Runs newest to oldest, the shown one filled; a
// click chooses a chip, a drag of the row chooses nothing, and the oldest end in view asks for
// older Runs once.
import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, onTestFinished, vi } from "vitest";
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

it("shows the Current chip first, then the Runs newest first, the shown one filled, and a click chooses one", async () => {
  const onchoose = vi.fn();
  const chips = [{ kind: "current" as const }, runChip("run-2", "GREEN"), runChip("run-1", "RED")];
  render(ChipRow, { branch: branch("main", chips), filled: 1, onchoose, onolder: vi.fn() });
  const buttons = within(row()).getAllByRole("button");
  expect(buttons.map((button) => button.textContent?.trim())).toEqual(["Current", "✓GREEN 5m", "✗RED 5m"]);
  expect(buttons.map((button) => button.getAttribute("aria-pressed"))).toEqual(["false", "true", "false"]);
  await fireEvent.click(buttons[0]);
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
  // jsdom lays nothing out: give the row a width, so its oldest end is in view.
  const laidOut = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(200);
  onTestFinished(() => laidOut.mockRestore());
  const onolder = vi.fn();
  render(ChipRow, { branch: branch("main", [runChip("run-1", "RED")], true), filled: null, onchoose: vi.fn(), onolder });
  expect(onolder).toHaveBeenCalledOnce();
  cleanup();
  const none = vi.fn();
  render(ChipRow, { branch: branch("main", [runChip("run-1", "RED")]), filled: null, onchoose: vi.fn(), onolder: none });
  expect(none).not.toHaveBeenCalled();
});
