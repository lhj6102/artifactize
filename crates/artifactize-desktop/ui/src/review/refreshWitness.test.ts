// E2E builds witness the Review list's manual refresh once its own reload succeeded and the
// list then settled on screen, whichever read the list shows: never for the clock redraw or a
// state-changed reload alone, nor for a manual reload that failed or has no answer yet. Read
// through the window's exposed activity, as E2E does; what the list shows is for the reader.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import RefreshHost from "./RefreshHost.svelte";
import { humanView } from "./fixtures";
import type { LaunchView, Listed } from "../ipc/client";

// The witness exists only in E2E builds; the mode is fixed before any module reads it.
vi.hoisted(() => { vi.stubEnv("MODE", "e2e"); });

const WAITING = "kind:human status:waiting_human";
const launch: LaunchView = { mode: "review", state: "/state", reviewer: "alice", search: WAITING, request: null };

const first: Listed = {
  id: "run-a-2",
  eval: "release/signoff",
  title: "A person approves the release",
  kind: "human",
  mark: { status: "WAITING_HUMAN", glyph: "?" },
  claim: "-",
  mine: false,
  age: "3s",
  repository: "/repo",
};
const second: Listed = { ...first, id: "run-b-2", eval: "docs/signoff", title: "A person approves the docs" };

type Read = { reload: boolean; answer: (rows: Listed[]) => Promise<void>; fail: (message: string) => Promise<void> };

/** Every list read waits until the test answers it, in the order the screen asked. */
function backend(): Read[] {
  const reads: Read[] = [];
  mockIPC(
    (cmd, args) => {
      const payload = args as Record<string, unknown>;
      switch (cmd) {
        case "review_search":
          return new Promise((resolve, reject) => reads.push({
            reload: payload.reload as boolean,
            answer: (rows) => completed(() => resolve({ generation: 0, rows, next: null, error: null, waiting: true })),
            fail: (message) => completed(() => reject({ kind: "read", message })),
          }));
        case "human_open":
          return humanView("claim");
        case "review_complete":
          return { keep: "", candidates: [] };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  return reads;
}

const witnessed = () => window.artifactizeCommands.reviewRefresh;

// Poll observed states in small controlled steps; only the tests below advance
// the one-second redraw clock. No fixed number of timer/tick flushes is a barrier.
const WAIT = { interval: 1, timeout: 1000 };

async function completed(answer: () => void): Promise<void> {
  const before = window.artifactizeCommands.commands.reviewSearch.completed;
  answer();
  await vi.waitFor(() => expect(window.artifactizeCommands.commands.reviewSearch.completed).toBeGreaterThan(before), WAIT);
  await tick();
}

async function generation(number: number): Promise<void> {
  await vi.waitFor(() => expect(witnessed()).toEqual({ generation: number, query: WAITING, start: 0 }), WAIT);
}

/** Wait for this actual native dispatch, not an assumed number of microtasks. */
async function next(reads: Read[], reload: boolean): Promise<Read> {
  await vi.waitFor(() => expect(reads[0]?.reload).toBe(reload), WAIT);
  return reads.shift()!;
}

async function mounted(reads: Read[]) {
  render(RefreshHost, { launch });
  await (await next(reads, true)).answer([first]);
  await vi.waitFor(() => expect(screen.getByText(first.eval)).toBeTruthy(), WAIT);
  return screen.getByRole("listbox", { name: "Requests" });
}

function selected(): string | null {
  return screen.getByRole("listbox", { name: "Requests" }).querySelector('[aria-selected="true"]')?.textContent ?? null;
}

beforeAll(() => {
  Element.prototype.scrollIntoView ??= () => {};
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
});

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.useRealTimers();
});

describe("the manual refresh witness", () => {
  it("advances once per manual refresh, from the list key or the palette, after its rows show", async () => {
    const reads = backend();
    const list = await mounted(reads);
    // The launch read, a state-changed reload and the clock redraw are not manual refreshes.
    expect(witnessed()).toBeNull();
    await emit("state-changed", { resync: false });
    await (await next(reads, true)).answer([first]);
    await vi.advanceTimersByTimeAsync(1000);
    await (await next(reads, false)).answer([first]);
    expect(witnessed()).toBeNull();

    await fireEvent.keyDown(list, { key: "r" });
    const manual = await next(reads, true);
    // Dispatched but unanswered is not done.
    expect(witnessed()).toBeNull();
    await manual.answer([second, first]);
    // The first moment the witness shows, the list already shows the rows it read.
    await generation(1);
    expect(screen.getByText(second.eval)).toBeTruthy();
    // The selection follows its request to its new place.
    expect(selected()).toContain(first.eval);

    await fireEvent.click(screen.getByRole("button", { name: "Commands" }));
    await fireEvent.click(await screen.findByRole("option", { name: /Refresh the list/ }));
    const palette = await next(reads, true);
    expect(witnessed()?.generation).toBe(1);
    await palette.answer([second]);
    await generation(2);
    expect(screen.queryByText(first.eval)).toBeNull();
    expect(Object.isFrozen(witnessed())).toBe(true);
  });

  it("never advances while its own reload failed or is unanswered, whatever other reads answer", async () => {
    const reads = backend();
    const list = await mounted(reads);
    await fireEvent.keyDown(list, { key: "r" });
    await (await next(reads, true)).fail("database is locked");
    await vi.waitFor(() => expect(screen.getByText("database is locked")).toBeTruthy(), WAIT);
    expect(witnessed()).toBeNull();

    await fireEvent.keyDown(list, { key: "r" });
    const manual = await next(reads, true);
    // A clock redraw and a state-changed reload both show a list; the refresh is not done.
    await vi.advanceTimersByTimeAsync(1000);
    await (await next(reads, false)).answer([first]);
    await emit("state-changed", { resync: false });
    await (await next(reads, true)).answer([second]);
    expect(screen.getByText(second.eval)).toBeTruthy();
    expect(witnessed()).toBeNull();
    // Its own reload failing late is still a failed refresh.
    await manual.fail("database is locked");
    expect(witnessed()).toBeNull();
  });

  it("advances once its own reload succeeded and the clock redraw that overtook it shows", async () => {
    const reads = backend();
    const list = await mounted(reads);
    // The clock redraw answers first: the list shows the backend's last page.
    await fireEvent.keyDown(list, { key: "r" });
    let manual = await next(reads, true);
    await vi.advanceTimersByTimeAsync(1000);
    await (await next(reads, false)).answer([first]);
    expect(witnessed()).toBeNull();
    await manual.answer([second, first]);
    await generation(1);
    // The witness does not say the list is fresh: it shows what the clock redraw read.
    expect(screen.queryByText(second.eval)).toBeNull();

    // Its own reload answers first: it waits for the overtaking redraw to show.
    await fireEvent.keyDown(list, { key: "r" });
    manual = await next(reads, true);
    await vi.advanceTimersByTimeAsync(1000);
    const clock = await next(reads, false);
    await manual.answer([second, first]);
    expect(witnessed()?.generation).toBe(1);
    await clock.answer([second, first]);
    await generation(2);
    expect(screen.getByText(second.eval)).toBeTruthy();
  });

  it("completes a manual refresh that a newer state-changed reload overtook, once that shows", async () => {
    const reads = backend();
    const list = await mounted(reads);
    await fireEvent.keyDown(list, { key: "r" });
    const manual = await next(reads, true);
    await emit("state-changed", { resync: false });
    const newer = await next(reads, true);
    await manual.answer([first]);
    // Its own answer is stale now; the refresh waits for the newer read.
    expect(witnessed()).toBeNull();
    await newer.answer([second]);
    await generation(1);
    expect(screen.getByText(second.eval)).toBeTruthy();
  });
});
