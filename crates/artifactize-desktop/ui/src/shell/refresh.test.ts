// E2E builds witness the window's manual refresh (F5, Ctrl+R or the palette's Refresh) once its
// own read succeeded and showed: never for the clock redraw or a state-changed read, nor for a
// manual read that failed or has no answer yet. Read through the window's exposed activity, as
// E2E does.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import App from "../App.svelte";
import { browseView } from "../monitor/fixtures";
import type { LaunchView, Navigate } from "../ipc/client";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {}, destroy: async () => {} }),
}));

// The witness exists only in E2E builds; the mode is fixed before any module reads it.
vi.hoisted(() => {
  vi.stubEnv("MODE", "e2e");
});

const launch: LaunchView = { mode: "monitor", state: "/state", reviewer: "alice", request: null };

type Read = { navigate: Navigate; answer: () => Promise<void>; fail: (message: string) => Promise<void> };

/** Every browse read waits until the test answers it, in the order the window asked. */
function backend(): Read[] {
  const reads: Read[] = [];
  let revision = 0;
  mockIPC(
    (cmd, args) => {
      switch (cmd) {
        case "launch":
          return launch;
        case "browse":
          return new Promise((resolve, reject) =>
            reads.push({
              navigate: (args as { navigate: Navigate }).navigate,
              answer: () => completed(() => resolve(browseView(++revision))),
              fail: (message) => completed(() => reject({ kind: "read", message })),
            }),
          );
        case "redraw":
          return browseView(revision);
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  return reads;
}

const witnessed = () => window.artifactizeCommands.refresh;

// Poll observed states in small controlled steps; only the tests advance the redraw clock.
const WAIT = { interval: 1, timeout: 1000 };

async function completed(answer: () => void): Promise<void> {
  const before = window.artifactizeCommands.commands.browse.completed;
  answer();
  await vi.waitFor(() => expect(window.artifactizeCommands.commands.browse.completed).toBeGreaterThan(before), WAIT);
}

/** Wait for this actual native dispatch, not an assumed number of microtasks. */
async function next(reads: Read[], resync: boolean): Promise<Read> {
  await vi.waitFor(() => expect(reads[0]?.navigate.resync).toBe(resync), WAIT);
  return reads.shift()!;
}

async function mounted(reads: Read[]) {
  render(App);
  await (await next(reads, false)).answer();
  await vi.waitFor(() => expect(screen.getByLabelText("Main area")).toBeTruthy(), WAIT);
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
  it("advances once per manual refresh, from F5 or the palette, after its read showed", async () => {
    const reads = backend();
    await mounted(reads);
    // The launch read, a state-changed read and the clock redraw are not manual refreshes.
    expect(witnessed()).toBeNull();
    await emit("state-changed", { resync: false });
    await (await next(reads, false)).answer();
    await vi.advanceTimersByTimeAsync(1000);
    expect(witnessed()).toBeNull();

    await fireEvent.keyDown(window, { key: "F5" });
    const manual = await next(reads, true);
    // Dispatched but unanswered is not done.
    expect(witnessed()).toBeNull();
    await manual.answer();
    await vi.waitFor(() => expect(witnessed()).toEqual({ generation: 1 }), WAIT);

    await fireEvent.click(screen.getByRole("button", { name: /Commands/ }));
    await fireEvent.click(await screen.findByRole("option", { name: /Refresh/ }));
    const palette = await next(reads, true);
    expect(witnessed()?.generation).toBe(1);
    await palette.answer();
    await vi.waitFor(() => expect(witnessed()).toEqual({ generation: 2 }), WAIT);
    expect(Object.isFrozen(witnessed())).toBe(true);
  });

  it("never advances while its own read failed or is unanswered, whatever other reads answer", async () => {
    const reads = backend();
    await mounted(reads);
    await fireEvent.keyDown(window, { key: "F5" });
    await (await next(reads, true)).fail("database is locked");
    await vi.waitFor(() => expect(screen.getByText("database is locked")).toBeTruthy(), WAIT);
    expect(witnessed()).toBeNull();

    await fireEvent.keyDown(window, { key: "F5" });
    const manual = await next(reads, true);
    // A state-changed read shows a snapshot; the refresh is not done.
    await emit("state-changed", { resync: false });
    await (await next(reads, false)).answer();
    expect(witnessed()).toBeNull();
    // Its own read failing late is still a failed refresh.
    await manual.fail("database is locked");
    expect(witnessed()).toBeNull();
  });
});
