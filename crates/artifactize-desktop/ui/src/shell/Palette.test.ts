// The palette's public keyboard boundary: input and Enter may arrive before a render.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Registry } from "./commands.svelte";
import Palette from "./Palette.svelte";

const commands = new Registry();
vi.mock("./app.svelte", () => ({ getAppState: () => ({ commands }) }));
const registrations: (() => void)[] = [];

beforeAll(() => {
  Element.prototype.scrollIntoView ??= () => {};
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(() => {
  cleanup();
  for (const remove of registrations.splice(0)) remove();
  vi.useRealTimers();
});

function setup() {
  const monitor = vi.fn();
  const review = vi.fn();
  registrations.push(commands.add(() => [
    { id: "app.monitor", group: "App", label: "Show Monitor", run: monitor },
    { id: "app.review", group: "App", label: "Show Review", run: review },
  ]));
  const view = render(Palette, { open: true });
  return { view, monitor, review };
}

async function focusedInput() {
  const field = await screen.findByRole<HTMLInputElement>("combobox");
  await waitFor(() => expect(document.activeElement).toBe(field));
  return field;
}

// Deliberately no tick between these native events: this is the race the user can hit.
function enterQuery(field: HTMLInputElement, query: string) {
  field.value = query;
  field.dispatchEvent(new Event("input", { bubbles: true }));
  field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
}

describe("command palette", () => {
  it("runs exactly the newly filtered command when input and Enter arrive together, after reopening too", async () => {
    const { view, monitor, review } = setup();
    for (const [label, invoked, count] of [
      ["Show Review", review, 1], ["Show Monitor", monitor, 1],
      ["Show Review", review, 2], ["Show Monitor", monitor, 2],
    ] as const) {
      enterQuery(await focusedInput(), label);
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
      await waitFor(() => expect(invoked).toHaveBeenCalledTimes(count));
      await view.rerender({ open: true });
    }
    expect(monitor).toHaveBeenCalledTimes(2);
    expect(review).toHaveBeenCalledTimes(2);
  });

  it("does not run the old selection when the new query has no matches", async () => {
    const { monitor, review } = setup();
    enterQuery(await focusedInput(), "no such command");
    await screen.findByText("No matching command.");
    await new Promise<void>((done) => requestAnimationFrame(() => done()));
    expect(screen.getByRole("dialog")).not.toBeNull();
    expect(monitor).not.toHaveBeenCalled();
    expect(review).not.toHaveBeenCalled();
  });

  it("keeps arrow selection and leaves composition Enter to the input method", async () => {
    const { monitor, review } = setup();
    const field = await focusedInput();
    await waitFor(() => expect(screen.getByRole("option", { name: "Show Monitor" }).getAttribute("aria-selected")).toBe("true"));
    await fireEvent.keyDown(field, { key: "ArrowDown" });
    await waitFor(() => expect(screen.getByRole("option", { name: "Show Review" }).getAttribute("aria-selected")).toBe("true"));
    await fireEvent.keyDown(field, { key: "Enter", isComposing: true });
    await fireEvent.keyDown(field, { key: "Enter", keyCode: 229 });
    expect(screen.getByRole("dialog")).not.toBeNull();
    expect(monitor).not.toHaveBeenCalled();
    expect(review).not.toHaveBeenCalled();
    await fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(review).toHaveBeenCalledTimes(1));
    expect(monitor).not.toHaveBeenCalled();
  });

  it("cancels a queued activation when Escape closes the palette before the render", async () => {
    const { monitor, review } = setup();
    const field = await focusedInput();
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    enterQuery(field, "Show Review");
    await fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    vi.advanceTimersToNextFrame();
    expect(monitor).not.toHaveBeenCalled();
    expect(review).not.toHaveBeenCalled();
  });
});
