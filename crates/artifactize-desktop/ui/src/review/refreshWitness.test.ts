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

type Read = { reload: boolean; answer: (rows: Listed[]) => void; fail: (message: string) => void };

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
            answer: (rows) => resolve({ generation: 0, rows, next: null, error: null, waiting: true }),
            fail: (message) => reject({ kind: "read", message }),
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

async function settle() {
  for (let round = 0; round < 3; round++) {
    await vi.advanceTimersByTimeAsync(0);
    await tick();
  }
}

/** The next list read the screen asked for. */
function next(reads: Read[], reload: boolean): Read {
  const read = reads.shift();
  expect(read?.reload).toBe(reload);
  return read!;
}

async function mounted(reads: Read[]) {
  render(RefreshHost, { launch });
  await settle();
  next(reads, true).answer([first]);
  await settle();
  expect(screen.getByText(first.eval)).toBeTruthy();
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
    await settle();
    next(reads, true).answer([first]);
    await vi.advanceTimersByTimeAsync(1000);
    next(reads, false).answer([first]);
    await settle();
    expect(witnessed()).toBeNull();

    await fireEvent.keyDown(list, { key: "r" });
    const manual = next(reads, true);
    await settle();
    // Dispatched but unanswered is not done.
    expect(witnessed()).toBeNull();
    manual.answer([second, first]);
    // The first moment the witness shows, the list already shows the rows it read.
    for (let turn = 0; turn < 100 && witnessed() === null; turn++) await Promise.resolve();
    expect(witnessed()).toEqual({ generation: 1, query: WAITING, start: 0 });
    expect(screen.getByText(second.eval)).toBeTruthy();
    // The selection follows its request to its new place.
    expect(selected()).toContain(first.eval);

    await fireEvent.click(screen.getByRole("button", { name: "Commands" }));
    await settle();
    await fireEvent.click(await screen.findByRole("option", { name: /Refresh the list/ }));
    await settle();
    const palette = next(reads, true);
    expect(witnessed()?.generation).toBe(1);
    palette.answer([second]);
    await settle();
    expect(witnessed()).toEqual({ generation: 2, query: WAITING, start: 0 });
    expect(screen.queryByText(first.eval)).toBeNull();
    expect(Object.isFrozen(witnessed())).toBe(true);
  });

  it("never advances while its own reload failed or is unanswered, whatever other reads answer", async () => {
    const reads = backend();
    const list = await mounted(reads);
    await fireEvent.keyDown(list, { key: "r" });
    next(reads, true).fail("database is locked");
    await settle();
    expect(screen.getByText("database is locked")).toBeTruthy();
    expect(witnessed()).toBeNull();

    await fireEvent.keyDown(list, { key: "r" });
    const manual = next(reads, true);
    // A clock redraw and a state-changed reload both show a list; the refresh is not done.
    await vi.advanceTimersByTimeAsync(1000);
    next(reads, false).answer([first]);
    await settle();
    await emit("state-changed", { resync: false });
    await settle();
    next(reads, true).answer([second]);
    await settle();
    expect(screen.getByText(second.eval)).toBeTruthy();
    expect(witnessed()).toBeNull();
    // Its own reload failing late is still a failed refresh.
    manual.fail("database is locked");
    await settle();
    expect(witnessed()).toBeNull();
  });

  it("advances once its own reload succeeded and the clock redraw that overtook it shows", async () => {
    const reads = backend();
    const list = await mounted(reads);
    // The clock redraw answers first: the list shows the backend's last page.
    await fireEvent.keyDown(list, { key: "r" });
    let manual = next(reads, true);
    await vi.advanceTimersByTimeAsync(1000);
    next(reads, false).answer([first]);
    await settle();
    expect(witnessed()).toBeNull();
    manual.answer([second, first]);
    await settle();
    expect(witnessed()).toEqual({ generation: 1, query: WAITING, start: 0 });
    // The witness does not say the list is fresh: it shows what the clock redraw read.
    expect(screen.queryByText(second.eval)).toBeNull();

    // Its own reload answers first: it waits for the overtaking redraw to show.
    await fireEvent.keyDown(list, { key: "r" });
    manual = next(reads, true);
    await vi.advanceTimersByTimeAsync(1000);
    const clock = next(reads, false);
    manual.answer([second, first]);
    await settle();
    expect(witnessed()?.generation).toBe(1);
    clock.answer([second, first]);
    await settle();
    expect(witnessed()).toEqual({ generation: 2, query: WAITING, start: 0 });
    expect(screen.getByText(second.eval)).toBeTruthy();
  });

  it("completes a manual refresh that a newer state-changed reload overtook, once that shows", async () => {
    const reads = backend();
    const list = await mounted(reads);
    await fireEvent.keyDown(list, { key: "r" });
    const manual = next(reads, true);
    await emit("state-changed", { resync: false });
    await settle();
    const newer = next(reads, true);
    manual.answer([first]);
    await settle();
    // Its own answer is stale now; the refresh waits for the newer read.
    expect(witnessed()).toBeNull();
    newer.answer([second]);
    await settle();
    expect(witnessed()).toEqual({ generation: 1, query: WAITING, start: 0 });
    expect(screen.getByText(second.eval)).toBeTruthy();
  });
});
